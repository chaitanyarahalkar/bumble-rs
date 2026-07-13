use crate::{CommandResponse, Error, PacketSink, Result, SplitOpenedTransport};
use bumble::keys::{KeyStore, PairingKeys};
use bumble::Address;
use bumble_att::AttPdu;
use bumble_controller::ROLE_CENTRAL;
use bumble_gatt::AttTransport;
use bumble_hci::metadata::supported_command_names;
use bumble_hci::{
    AclDataPacket, Command, Event, HciPacket, IsoDataPacket, ReturnParameters,
    SynchronousDataPacket,
};
use bumble_host::{Device, HostTransport};
use bumble_smp::{
    security_request, AcceptAllDelegate, AuthReq, ManagedPairingState, PairingConfig,
    PairingConnection, PairingDelegateFactory, PairingManager, PairingRole, PairingState,
    ScPairingState, SmpPdu, SMP_CID,
};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalHostState {
    Running,
    Ended,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalHostActivity {
    Packet,
    Timeout,
    Ended,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalControllerInfo {
    pub supported_commands: [u8; 64],
    pub acl_data_packet_length: u16,
    pub total_num_acl_data_packets: u16,
    pub le_acl_data_packet_length: u16,
    pub total_num_le_acl_data_packets: u8,
    pub iso_data_packet_length: u16,
    pub total_num_iso_data_packets: u8,
}

/// Transport-neutral LE SMP orchestration for a live [`Device`] connection.
///
/// The cryptographic and protocol state remains owned by
/// [`bumble_smp::PairingManager`]. This adapter only moves SMP PDUs over the
/// fixed L2CAP channel and translates the controller encryption handshake into
/// pairing-manager lifecycle events.
pub struct LePairingSession {
    manager: PairingManager,
    connection_handle: u16,
    role: PairingRole,
    auth_req: AuthReq,
    started: bool,
    encryption_started: bool,
    marked_encrypted: bool,
}

impl LePairingSession {
    pub fn new(
        device: &Device,
        connection_handle: u16,
        local_address: Address,
        config: PairingConfig,
        delegate_factory: PairingDelegateFactory,
    ) -> Result<Self> {
        let connection = device.le_connection(connection_handle).ok_or_else(|| {
            Error::Remote(format!(
                "unknown LE connection handle {connection_handle:#06x}"
            ))
        })?;
        let role = if connection.role == ROLE_CENTRAL {
            PairingRole::Initiator
        } else {
            PairingRole::Responder
        };
        let auth_req = AuthReq::from_booleans(
            config.bonding,
            config.secure_connections,
            config.mitm,
            false,
            config.ct2,
        );
        let mut manager = PairingManager::new(config, delegate_factory);
        manager
            .register_connection(PairingConnection::le(
                connection_handle,
                role,
                local_address,
                connection.peer_address.clone(),
            ))
            .map_err(|error| Error::Remote(error.to_string()))?;
        Ok(Self {
            manager,
            connection_handle,
            role,
            auth_req,
            started: false,
            encryption_started: false,
            marked_encrypted: false,
        })
    }

    pub fn accept_all(
        device: &Device,
        connection_handle: u16,
        local_address: Address,
        config: PairingConfig,
    ) -> Result<Self> {
        Self::new(
            device,
            connection_handle,
            local_address,
            config,
            Box::new(|_, _| Box::new(AcceptAllDelegate)),
        )
    }

    pub fn connection_handle(&self) -> u16 {
        self.connection_handle
    }

    pub fn role(&self) -> PairingRole {
        self.role
    }

    pub fn state(&self) -> Option<ManagedPairingState> {
        self.manager.state(self.connection_handle)
    }

    /// Start pairing. A central emits Pairing Request; a peripheral emits
    /// Security Request so that its central peer starts the feature exchange.
    pub fn begin(&mut self, link: &mut bumble_host::LocalLink, device: &mut Device) -> Result<()> {
        if self.started {
            return Err(Error::Remote("pairing session already started".into()));
        }
        if !device.is_connected_on_handle(self.connection_handle) {
            return Err(Error::Remote(format!(
                "LE connection {:#06x} is not active",
                self.connection_handle
            )));
        }
        match self.role {
            PairingRole::Initiator => self
                .manager
                .pair(self.connection_handle)
                .map_err(|error| Error::Remote(error.to_string()))?,
            PairingRole::Responder => {
                if !device.send_l2cap_on_handle(
                    link,
                    self.connection_handle,
                    SMP_CID,
                    &security_request(self.auth_req).to_bytes(),
                ) {
                    return Err(Error::Remote(format!(
                        "failed to send SMP Security Request on handle {:#06x}",
                        self.connection_handle
                    )));
                }
            }
        }
        self.started = true;
        Ok(())
    }

    /// Advance pairing without blocking. Returns the completed keys once key
    /// distribution has finished.
    pub fn drive_once(
        &mut self,
        link: &mut bumble_host::LocalLink,
        device: &mut Device,
    ) -> Result<Option<PairingKeys>> {
        if !self.started {
            return Err(Error::Remote("pairing session has not been started".into()));
        }
        if !device.is_connected_on_handle(self.connection_handle) {
            return Err(Error::Remote(format!(
                "LE connection {:#06x} ended during pairing",
                self.connection_handle
            )));
        }

        self.flush_outbound(link, device)?;
        device.poll(link);
        for payload in device.take_l2cap_on_handle(self.connection_handle, SMP_CID) {
            let pdu =
                SmpPdu::from_bytes(&payload).map_err(|error| Error::Remote(error.to_string()))?;
            self.manager
                .receive(self.connection_handle, pdu)
                .map_err(|error| Error::Remote(error.to_string()))?;
        }

        while self.manager.poll_security_request().is_some() {
            if self.role == PairingRole::Initiator
                && self.manager.state(self.connection_handle).is_none()
            {
                self.manager
                    .pair(self.connection_handle)
                    .map_err(|error| Error::Remote(error.to_string()))?;
            }
        }

        for request in device.take_long_term_key_requests_on_handle(self.connection_handle) {
            let Some(key) = self.manager.encryption_key(self.connection_handle) else {
                device.reject_long_term_key_request(link, request.connection_handle);
                return Err(Error::Remote(format!(
                    "controller requested an LTK before pairing produced one on handle {:#06x}",
                    request.connection_handle
                )));
            };
            if !device.reply_long_term_key_request(link, request.connection_handle, key) {
                return Err(Error::Remote(format!(
                    "failed to answer controller LTK request on handle {:#06x}",
                    request.connection_handle
                )));
            }
        }

        if self.waiting_for_encryption() {
            if self.role == PairingRole::Initiator && !self.encryption_started {
                let key = self
                    .manager
                    .encryption_key(self.connection_handle)
                    .ok_or_else(|| {
                        Error::Remote("pairing reached encryption without a key".into())
                    })?;
                if !device.enable_encryption_on_handle(link, self.connection_handle, key) {
                    return Err(Error::Remote(format!(
                        "failed to start encryption on handle {:#06x}",
                        self.connection_handle
                    )));
                }
                self.encryption_started = true;
            }
            if device.is_encrypted_on_handle(self.connection_handle) && !self.marked_encrypted {
                self.manager
                    .mark_encrypted(self.connection_handle)
                    .map_err(|error| Error::Remote(error.to_string()))?;
                self.marked_encrypted = true;
            }
        }

        self.flush_outbound(link, device)?;
        if self.failed() {
            return Err(Error::Remote(format!(
                "SMP pairing failed: {:?}",
                self.manager.failure(self.connection_handle)
            )));
        }
        if self.complete() {
            return self
                .manager
                .pairing_keys(self.connection_handle)
                .map(Some)
                .ok_or_else(|| Error::Remote("pairing completed without keys".into()));
        }
        Ok(None)
    }

    /// Run pairing to completion over an external HCI transport.
    pub fn pair(
        &mut self,
        host: &mut ExternalHost,
        device: &mut Device,
        timeout: Duration,
    ) -> Result<PairingKeys> {
        self.begin(host, device)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(keys) = self.drive_once(host, device)? {
                return Ok(keys);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Remote(format!(
                    "timed out pairing LE connection {:#06x}",
                    self.connection_handle
                )));
            }
            match host.wait_for_activity(remaining)? {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout => {
                    return Err(Error::Remote(format!(
                        "timed out pairing LE connection {:#06x}",
                        self.connection_handle
                    )))
                }
                ExternalHostActivity::Ended => {
                    return Err(Error::Remote(format!(
                        "transport ended while pairing LE connection {:#06x}",
                        self.connection_handle
                    )))
                }
            }
        }
    }

    pub fn store_bond(&self, store: &mut dyn KeyStore) -> Result<bool> {
        self.manager
            .store_bond(self.connection_handle, store)
            .map_err(|error| Error::Remote(error.to_string()))
    }

    fn flush_outbound(
        &mut self,
        link: &mut bumble_host::LocalLink,
        device: &mut Device,
    ) -> Result<()> {
        for (handle, pdu) in self.manager.drain_outbound() {
            if !device.send_l2cap_on_handle(link, handle, SMP_CID, &pdu.to_bytes()) {
                return Err(Error::Remote(format!(
                    "failed to send SMP PDU on handle {handle:#06x}"
                )));
            }
        }
        Ok(())
    }

    fn waiting_for_encryption(&self) -> bool {
        matches!(
            self.state(),
            Some(ManagedPairingState::Legacy(PairingState::WaitEncryption))
                | Some(ManagedPairingState::SecureConnections(
                    ScPairingState::WaitEncryption
                ))
        )
    }

    fn complete(&self) -> bool {
        matches!(
            self.state(),
            Some(ManagedPairingState::Legacy(PairingState::Complete))
                | Some(ManagedPairingState::SecureConnections(
                    ScPairingState::Complete
                ))
        )
    }

    fn failed(&self) -> bool {
        matches!(
            self.state(),
            Some(ManagedPairingState::Legacy(PairingState::Failed))
                | Some(ManagedPairingState::SecureConnections(
                    ScPairingState::Failed
                ))
        )
    }
}

/// Synchronous ATT bearer over an initialized [`ExternalHost`] and connected
/// [`Device`].
pub struct ExternalAttTransport<'a> {
    host: &'a mut ExternalHost,
    device: &'a mut Device,
    connection_handle: u16,
    timeout: Duration,
    unsolicited: VecDeque<AttPdu>,
}

impl<'a> ExternalAttTransport<'a> {
    pub fn new(
        host: &'a mut ExternalHost,
        device: &'a mut Device,
        connection_handle: u16,
        timeout: Duration,
    ) -> Result<Self> {
        if !device.is_connected_on_handle(connection_handle) {
            return Err(Error::Remote(format!(
                "unknown LE connection handle {connection_handle:#06x}"
            )));
        }
        Ok(Self {
            host,
            device,
            connection_handle,
            timeout,
            unsolicited: VecDeque::new(),
        })
    }

    pub fn take_unsolicited(&mut self) -> Vec<AttPdu> {
        self.unsolicited.drain(..).collect()
    }

    fn take_response(&mut self, request_opcode: u8) -> Option<AttPdu> {
        let mut response = None;
        for pdu in self.device.take_inbox_on_handle(self.connection_handle) {
            let matches = match &pdu {
                AttPdu::ErrorResponse {
                    request_opcode_in_error,
                    ..
                } => *request_opcode_in_error == request_opcode,
                _ => pdu.op_code() == request_opcode.wrapping_add(1),
            };
            if response.is_none() && matches {
                response = Some(pdu);
            } else {
                self.unsolicited.push_back(pdu);
            }
        }
        response
    }

    fn request_result(&mut self, request: &AttPdu) -> Result<AttPdu> {
        if !self
            .device
            .send_att_on_handle(self.host, self.connection_handle, request)
        {
            return Err(Error::Remote(format!(
                "failed to send ATT request {:#04x} on handle {:#06x}",
                request.op_code(),
                self.connection_handle
            )));
        }
        if request.is_command() {
            return Ok(AttPdu::WriteResponse);
        }

        let deadline = Instant::now() + self.timeout;
        loop {
            self.device.poll(self.host);
            if let Some(response) = self.take_response(request.op_code()) {
                return Ok(response);
            }
            if !self.device.is_connected_on_handle(self.connection_handle) {
                return Err(Error::Remote(format!(
                    "LE connection {:#06x} ended before ATT response {:#04x}",
                    self.connection_handle,
                    request.op_code()
                )));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Remote(format!(
                    "timed out waiting for ATT response to {:#04x}",
                    request.op_code()
                )));
            }
            match self.host.wait_for_activity(remaining)? {
                ExternalHostActivity::Packet => {}
                ExternalHostActivity::Timeout => {
                    return Err(Error::Remote(format!(
                        "timed out waiting for ATT response to {:#04x}",
                        request.op_code()
                    )))
                }
                ExternalHostActivity::Ended => {
                    return Err(Error::Remote(format!(
                        "transport ended before ATT response to {:#04x}",
                        request.op_code()
                    )))
                }
            }
        }
    }
}

impl AttTransport for ExternalAttTransport<'_> {
    fn request(&mut self, request: &AttPdu) -> AttPdu {
        self.request_result(request)
            .unwrap_or_else(|_| AttPdu::ErrorResponse {
                request_opcode_in_error: request.op_code(),
                attribute_handle_in_error: 0,
                error_code: 0x0E,
            })
    }

    fn try_request(&mut self, request: &AttPdu) -> core::result::Result<AttPdu, String> {
        self.request_result(request)
            .map_err(|error| error.to_string())
    }
}

enum ReaderMessage {
    Packet(Box<HciPacket>),
    Ended,
    Failed(String),
}

/// A host-side HCI adapter backed by an independently owned packet source and
/// sink.
///
/// A reader worker waits on the blocking source while callers retain exclusive
/// ownership of the sink and [`bumble_host::Device`]. Incoming packets are
/// collected non-blockingly through [`HostTransport::drain_host_events`], or a
/// caller can use [`ExternalHost::wait_for_activity`] to efficiently drive a
/// synchronous application loop.
pub struct ExternalHost {
    sink: Box<dyn PacketSink + Send>,
    receiver: Receiver<ReaderMessage>,
    pending: VecDeque<HciPacket>,
    state: ExternalHostState,
}

impl ExternalHost {
    pub fn new(transport: SplitOpenedTransport) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut source = transport.source;
        std::thread::spawn(move || loop {
            match source.read_packet() {
                Ok(Some(packet)) => {
                    if sender
                        .send(ReaderMessage::Packet(Box::new(packet)))
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = sender.send(ReaderMessage::Ended);
                    return;
                }
                Err(error) => {
                    let _ = sender.send(ReaderMessage::Failed(error.to_string()));
                    return;
                }
            }
        });
        Self {
            sink: transport.sink,
            receiver,
            pending: VecDeque::new(),
            state: ExternalHostState::Running,
        }
    }

    pub fn state(&self) -> &ExternalHostState {
        &self.state
    }

    pub fn wait_for_activity(&mut self, timeout: Duration) -> Result<ExternalHostActivity> {
        if !self.pending.is_empty() {
            return Ok(ExternalHostActivity::Packet);
        }
        match &self.state {
            ExternalHostState::Ended => return Ok(ExternalHostActivity::Ended),
            ExternalHostState::Failed(message) => return Err(Error::Remote(message.clone())),
            ExternalHostState::Running => {}
        }
        match self.receiver.recv_timeout(timeout) {
            Ok(message) => self.receive_message(message),
            Err(RecvTimeoutError::Timeout) => Ok(ExternalHostActivity::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                self.state = ExternalHostState::Ended;
                Ok(ExternalHostActivity::Ended)
            }
        }
    }

    /// Send one HCI command and wait for its matching Command Complete or
    /// Command Status event. Unrelated asynchronous packets remain queued for
    /// the attached [`Device`].
    pub fn send_command(&mut self, command: Command, timeout: Duration) -> Result<CommandResponse> {
        let expected_opcode = command.op_code();
        if !self.write(0, HciPacket::Command(command)) {
            return Err(
                self.failure_error(format!("failed to send HCI command {expected_opcode:#06x}"))
            );
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Remote(format!(
                    "timed out waiting for HCI command {expected_opcode:#06x}"
                )));
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(ReaderMessage::Packet(packet)) => match *packet {
                    HciPacket::Event(Event::CommandComplete {
                        num_hci_command_packets,
                        command_opcode,
                        return_parameters,
                    }) if command_opcode == expected_opcode => {
                        return Ok(CommandResponse::Complete {
                            num_hci_command_packets,
                            return_parameters,
                        });
                    }
                    HciPacket::Event(Event::CommandStatus {
                        status,
                        num_hci_command_packets,
                        command_opcode,
                    }) if command_opcode == expected_opcode => {
                        return Ok(CommandResponse::Status {
                            status,
                            num_hci_command_packets,
                        });
                    }
                    packet => self.pending.push_back(packet),
                },
                Ok(ReaderMessage::Ended) => {
                    self.state = ExternalHostState::Ended;
                    return Err(Error::Remote(format!(
                        "transport ended before response to HCI command {expected_opcode:#06x}"
                    )));
                }
                Ok(ReaderMessage::Failed(message)) => {
                    self.state = ExternalHostState::Failed(message.clone());
                    return Err(Error::Remote(message));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error::Remote(format!(
                        "timed out waiting for HCI command {expected_opcode:#06x}"
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.state = ExternalHostState::Ended;
                    return Err(Error::Remote(format!(
                        "transport ended before response to HCI command {expected_opcode:#06x}"
                    )));
                }
            }
        }
    }

    /// Reset and configure an external controller, then apply its LE ACL flow
    /// control limits to `device`.
    pub fn initialize_device(
        &mut self,
        device: &mut Device,
        timeout: Duration,
    ) -> Result<ExternalControllerInfo> {
        self.send_successful_command(Command::Reset, timeout)?;
        let supported_commands =
            match self.send_successful_command(Command::ReadLocalSupportedCommands, timeout)? {
                ReturnParameters::ReadLocalSupportedCommands {
                    supported_commands, ..
                } => supported_commands,
                response => {
                    return Err(Error::Remote(format!(
                        "unexpected Read Local Supported Commands response: {response:?}"
                    )))
                }
            };
        let supported_names = supported_command_names(&supported_commands);

        self.send_successful_command(
            Command::SetEventMask {
                event_mask: event_mask(&[0x05, 0x08, 0x10, 0x13, 0x1A, 0x30, 0x3E]),
            },
            timeout,
        )?;
        if supported_names.contains(&"HCI_LE_SET_EVENT_MASK_COMMAND") {
            self.send_successful_command(
                Command::LeSetEventMask {
                    le_event_mask: event_mask(&[
                        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x0A, 0x0C, 0x0D, 0x0E, 0x0F,
                        0x10, 0x18, 0x19, 0x1A, 0x22, 0x23, 0x24, 0x25, 0x26, 0x29,
                    ]),
                },
                timeout,
            )?;
        }

        let mut info = ExternalControllerInfo {
            supported_commands,
            acl_data_packet_length: 0,
            total_num_acl_data_packets: 0,
            le_acl_data_packet_length: 0,
            total_num_le_acl_data_packets: 0,
            iso_data_packet_length: 0,
            total_num_iso_data_packets: 0,
        };
        if supported_names.contains(&"HCI_READ_BUFFER_SIZE_COMMAND") {
            match self.send_successful_command(Command::ReadBufferSize, timeout)? {
                ReturnParameters::ReadBufferSize {
                    hc_acl_data_packet_length,
                    hc_total_num_acl_data_packets,
                    ..
                } => {
                    info.acl_data_packet_length = hc_acl_data_packet_length;
                    info.total_num_acl_data_packets = hc_total_num_acl_data_packets;
                }
                response => {
                    return Err(Error::Remote(format!(
                        "unexpected Read Buffer Size response: {response:?}"
                    )))
                }
            }
        }
        if supported_names.contains(&"HCI_LE_READ_BUFFER_SIZE_V2_COMMAND") {
            match self.send_successful_command(Command::LeReadBufferSizeV2, timeout)? {
                ReturnParameters::LeReadBufferSizeV2 {
                    le_acl_data_packet_length,
                    total_num_le_acl_data_packets,
                    iso_data_packet_length,
                    total_num_iso_data_packets,
                    ..
                } => {
                    info.le_acl_data_packet_length = le_acl_data_packet_length;
                    info.total_num_le_acl_data_packets = total_num_le_acl_data_packets;
                    info.iso_data_packet_length = iso_data_packet_length;
                    info.total_num_iso_data_packets = total_num_iso_data_packets;
                }
                response => {
                    return Err(Error::Remote(format!(
                        "unexpected LE Read Buffer Size V2 response: {response:?}"
                    )))
                }
            }
        } else if supported_names.contains(&"HCI_LE_READ_BUFFER_SIZE_COMMAND") {
            match self.send_successful_command(Command::LeReadBufferSize, timeout)? {
                ReturnParameters::LeReadBufferSize {
                    le_acl_data_packet_length,
                    total_num_le_acl_data_packets,
                    ..
                } => {
                    info.le_acl_data_packet_length = le_acl_data_packet_length;
                    info.total_num_le_acl_data_packets = total_num_le_acl_data_packets;
                }
                response => {
                    return Err(Error::Remote(format!(
                        "unexpected LE Read Buffer Size response: {response:?}"
                    )))
                }
            }
        }

        let (packet_length, packet_count) =
            if info.le_acl_data_packet_length != 0 && info.total_num_le_acl_data_packets != 0 {
                (
                    info.le_acl_data_packet_length,
                    usize::from(info.total_num_le_acl_data_packets),
                )
            } else {
                (
                    info.acl_data_packet_length,
                    usize::from(info.total_num_acl_data_packets),
                )
            };
        if packet_length != 0 && !device.set_acl_data_packet_length(usize::from(packet_length)) {
            return Err(Error::Remote(format!(
                "invalid controller ACL packet length {packet_length}"
            )));
        }
        if packet_count != 0 && !device.set_acl_max_in_flight(packet_count) {
            return Err(Error::Remote(format!(
                "invalid controller ACL packet count {packet_count}"
            )));
        }
        Ok(info)
    }

    fn send_successful_command(
        &mut self,
        command: Command,
        timeout: Duration,
    ) -> Result<ReturnParameters> {
        let opcode = command.op_code();
        let response = self.send_command(command, timeout)?;
        if response.status() != Some(0) {
            return Err(Error::Remote(format!(
                "HCI command {opcode:#06x} failed with status {:?}",
                response.status()
            )));
        }
        response.return_parameters().cloned().ok_or_else(|| {
            Error::Remote(format!(
                "HCI command {opcode:#06x} returned Command Status instead of Command Complete"
            ))
        })
    }

    fn receive_message(&mut self, message: ReaderMessage) -> Result<ExternalHostActivity> {
        match message {
            ReaderMessage::Packet(packet) => {
                self.pending.push_back(*packet);
                Ok(ExternalHostActivity::Packet)
            }
            ReaderMessage::Ended => {
                self.state = ExternalHostState::Ended;
                Ok(ExternalHostActivity::Ended)
            }
            ReaderMessage::Failed(message) => {
                self.state = ExternalHostState::Failed(message.clone());
                Err(Error::Remote(message))
            }
        }
    }

    fn collect_available(&mut self) {
        while matches!(self.state, ExternalHostState::Running) {
            match self.receiver.try_recv() {
                Ok(message) => {
                    let _ = self.receive_message(message);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.state = ExternalHostState::Ended;
                    return;
                }
            }
        }
    }

    fn fail(&mut self, message: impl Into<String>) {
        self.state = ExternalHostState::Failed(message.into());
    }

    fn failure_error(&self, fallback: String) -> Error {
        match &self.state {
            ExternalHostState::Failed(message) => Error::Remote(message.clone()),
            ExternalHostState::Ended => Error::Remote("transport has ended".into()),
            ExternalHostState::Running => Error::Remote(fallback),
        }
    }

    fn write(&mut self, controller_id: usize, packet: HciPacket) -> bool {
        if controller_id != 0 {
            self.fail(format!(
                "external host exposes controller 0, not controller {controller_id}"
            ));
            return false;
        }
        if !matches!(self.state, ExternalHostState::Running) {
            return false;
        }
        if let Err(error) = self
            .sink
            .write_packet(&packet)
            .and_then(|()| self.sink.flush())
        {
            self.fail(error.to_string());
            return false;
        }
        true
    }
}

fn event_mask(event_codes: &[u8]) -> [u8; 8] {
    let mut mask = [0; 8];
    for event_code in event_codes
        .iter()
        .copied()
        .filter(|code| (1..=64).contains(code))
    {
        let bit = usize::from(event_code - 1);
        mask[bit / 8] |= 1 << (bit % 8);
    }
    mask
}

impl HostTransport for ExternalHost {
    fn handle_command(&mut self, controller_id: usize, command: Command) {
        self.write(controller_id, HciPacket::Command(command));
    }

    fn send_acl_packet(&mut self, controller_id: usize, packet: AclDataPacket) -> bool {
        self.write(controller_id, HciPacket::AclData(packet))
    }

    fn send_synchronous_data(
        &mut self,
        controller_id: usize,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        let Ok(data_total_length) = u8::try_from(data.len()) else {
            return false;
        };
        self.write(
            controller_id,
            HciPacket::SyncData(SynchronousDataPacket {
                connection_handle,
                packet_status,
                data_total_length,
                data: data.to_vec(),
            }),
        )
    }

    fn send_iso_packet(&mut self, controller_id: usize, packet: IsoDataPacket) -> bool {
        self.write(controller_id, HciPacket::IsoData(packet))
    }

    fn drain_host_events(&mut self, controller_id: usize) -> Vec<HciPacket> {
        if controller_id != 0 {
            self.fail(format!(
                "external host exposes controller 0, not controller {controller_id}"
            ));
            return Vec::new();
        }
        self.collect_available();
        self.pending.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PacketSource, Result as TransportResult};
    use bumble::Address;
    use bumble_hci::{CustomPacket, Event, LeMetaEvent};
    use bumble_host::Device;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    struct ScriptedSource(VecDeque<TransportResult<Option<HciPacket>>>);

    impl PacketSource for ScriptedSource {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            self.0.pop_front().unwrap_or(Ok(None))
        }
    }

    struct ChannelSource(std::sync::mpsc::Receiver<HciPacket>);

    impl PacketSource for ChannelSource {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            Ok(self.0.recv().ok())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<HciPacket>>>);

    impl PacketSink for RecordingSink {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            self.0.lock().unwrap().push(packet.clone());
            Ok(())
        }
    }

    struct FailingSink;

    impl PacketSink for FailingSink {
        fn write_packet(&mut self, _packet: &HciPacket) -> TransportResult<()> {
            Err(Error::Remote("write failed".into()))
        }
    }

    fn split(incoming: Vec<HciPacket>, sink: RecordingSink) -> SplitOpenedTransport {
        let mut script = incoming
            .into_iter()
            .map(|packet| Ok(Some(packet)))
            .collect::<VecDeque<_>>();
        script.push_back(Ok(None));
        SplitOpenedTransport {
            source: Box::new(ScriptedSource(script)),
            sink: Box::new(sink),
            metadata: BTreeMap::new(),
        }
    }

    fn command_complete(command: Command, return_parameters: ReturnParameters) -> HciPacket {
        HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: command.op_code(),
            return_parameters,
        })
    }

    fn att_acl(connection_handle: u16, pdu: AttPdu) -> HciPacket {
        let payload = pdu.to_bytes();
        let mut data = Vec::with_capacity(4 + payload.len());
        data.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        data.extend_from_slice(&bumble_host::ATT_CID.to_le_bytes());
        data.extend_from_slice(&payload);
        HciPacket::AclData(AclDataPacket {
            connection_handle,
            pb_flag: 0,
            bc_flag: 0,
            data_total_length: data.len() as u16,
            data,
        })
    }

    #[test]
    fn sends_typed_packets_and_collects_reader_packets() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let incoming = HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle: 0x123,
            role: 0,
            peer_address_type: 1,
            peer_address: address,
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 42,
            central_clock_accuracy: 0,
        }));
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(split(vec![incoming.clone()], sink));

        host.handle_command(0, Command::Reset);
        assert!(host.send_acl_packet(
            0,
            AclDataPacket {
                connection_handle: 0x123,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: 2,
                data: vec![1, 2],
            }
        ));
        assert_eq!(
            recorded.0.lock().unwrap().as_slice(),
            &[
                HciPacket::Command(Command::Reset),
                HciPacket::AclData(AclDataPacket {
                    connection_handle: 0x123,
                    pb_flag: 0,
                    bc_flag: 0,
                    data_total_length: 2,
                    data: vec![1, 2],
                }),
            ]
        );
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert_eq!(host.drain_host_events(0), vec![incoming]);
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Ended
        );
    }

    #[test]
    fn rejects_nonzero_controller_and_oversized_synchronous_data() {
        let sink = RecordingSink::default();
        let mut host = ExternalHost::new(split(Vec::new(), sink));
        assert!(!host.send_acl_packet(
            1,
            AclDataPacket {
                connection_handle: 0,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: 0,
                data: Vec::new(),
            }
        ));
        assert!(matches!(host.state(), ExternalHostState::Failed(_)));

        let sink = RecordingSink::default();
        let mut host = ExternalHost::new(split(Vec::new(), sink));
        assert!(!host.send_synchronous_data(0, 1, 0, &[0; 256]));
    }

    #[test]
    fn preserves_reader_and_writer_failures() {
        let read_transport = SplitOpenedTransport {
            source: Box::new(ScriptedSource(VecDeque::from([Err(Error::Remote(
                "read failed".into(),
            ))]))),
            sink: Box::new(RecordingSink::default()),
            metadata: BTreeMap::new(),
        };
        let mut host = ExternalHost::new(read_transport);
        assert!(host.wait_for_activity(Duration::from_secs(1)).is_err());
        assert_eq!(
            host.state(),
            &ExternalHostState::Failed("remote transport error: read failed".into())
        );

        let write_transport = SplitOpenedTransport {
            source: Box::new(ScriptedSource(VecDeque::new())),
            sink: Box::new(FailingSink),
            metadata: BTreeMap::new(),
        };
        let mut host = ExternalHost::new(write_transport);
        host.handle_command(0, Command::Reset);
        assert_eq!(
            host.state(),
            &ExternalHostState::Failed("remote transport error: write failed".into())
        );
    }

    #[test]
    fn command_wait_preserves_interleaved_packets() {
        let unrelated = HciPacket::Custom(CustomPacket::new(vec![0xAA, 0xBB]));
        let sink = RecordingSink::default();
        let mut host = ExternalHost::new(split(
            vec![
                unrelated.clone(),
                command_complete(Command::Reset, ReturnParameters::Status { status: 0 }),
            ],
            sink,
        ));

        assert_eq!(
            host.send_command(Command::Reset, Duration::from_secs(1))
                .unwrap()
                .status(),
            Some(0)
        );
        assert_eq!(host.drain_host_events(0), vec![unrelated]);
    }

    #[test]
    fn initializes_controller_and_device_acl_flow_control() {
        let mut supported_commands = [0; 64];
        supported_commands[14] = 0x80;
        supported_commands[25] = 0x03;
        supported_commands[41] = 0x20;
        let responses = vec![
            command_complete(Command::Reset, ReturnParameters::Status { status: 0 }),
            command_complete(
                Command::ReadLocalSupportedCommands,
                ReturnParameters::ReadLocalSupportedCommands {
                    status: 0,
                    supported_commands,
                },
            ),
            command_complete(
                Command::SetEventMask { event_mask: [0; 8] },
                ReturnParameters::Status { status: 0 },
            ),
            command_complete(
                Command::LeSetEventMask {
                    le_event_mask: [0; 8],
                },
                ReturnParameters::Status { status: 0 },
            ),
            command_complete(
                Command::ReadBufferSize,
                ReturnParameters::ReadBufferSize {
                    status: 0,
                    hc_acl_data_packet_length: 1021,
                    hc_synchronous_data_packet_length: 64,
                    hc_total_num_acl_data_packets: 8,
                    hc_total_num_synchronous_data_packets: 4,
                },
            ),
            command_complete(
                Command::LeReadBufferSizeV2,
                ReturnParameters::LeReadBufferSizeV2 {
                    status: 0,
                    le_acl_data_packet_length: 251,
                    total_num_le_acl_data_packets: 12,
                    iso_data_packet_length: 120,
                    total_num_iso_data_packets: 6,
                },
            ),
        ];
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(split(responses, sink));
        let mut device = Device::new(0);

        let info = host
            .initialize_device(&mut device, Duration::from_secs(1))
            .unwrap();
        assert_eq!(info.le_acl_data_packet_length, 251);
        assert_eq!(info.total_num_le_acl_data_packets, 12);
        assert_eq!(info.iso_data_packet_length, 120);
        assert_eq!(device.acl_data_packet_length(), 251);
        assert_eq!(device.acl_max_in_flight(), 12);
        assert_eq!(
            recorded
                .0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|packet| match packet {
                    HciPacket::Command(command) => Some(command.op_code()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![
                Command::Reset.op_code(),
                Command::ReadLocalSupportedCommands.op_code(),
                Command::SetEventMask { event_mask: [0; 8] }.op_code(),
                Command::LeSetEventMask {
                    le_event_mask: [0; 8]
                }
                .op_code(),
                Command::ReadBufferSize.op_code(),
                Command::LeReadBufferSizeV2.op_code(),
            ]
        );
    }

    #[test]
    fn drives_device_connection_state_over_external_hci() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let incoming = HciPacket::Event(Event::LeMeta(LeMetaEvent::EnhancedConnectionComplete {
            status: 0,
            connection_handle: 0x123,
            role: 0,
            peer_address_type: 1,
            peer_address: address.clone(),
            local_resolvable_private_address: address.clone(),
            peer_resolvable_private_address: address.clone(),
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 42,
            central_clock_accuracy: 0,
        }));
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(split(vec![incoming], sink));
        let mut device = Device::new(0);

        device.connect_le(&mut host, address.clone());
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert!(device.poll(&mut host));
        assert_eq!(device.connection_handle(), Some(0x123));
        assert_eq!(device.peer_address(), Some(&address));
        assert!(matches!(
            recorded.0.lock().unwrap().first(),
            Some(HciPacket::Command(Command::LeCreateConnection { peer_address, .. }))
                if peer_address == &address
        ));
    }

    #[test]
    fn external_att_transport_returns_response_and_retains_notification() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let connection = HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            status: 0,
            connection_handle: 0x123,
            role: 0,
            peer_address_type: 1,
            peer_address: address,
            connection_interval: 24,
            peripheral_latency: 0,
            supervision_timeout: 42,
            central_clock_accuracy: 0,
        }));
        let (sender, receiver) = std::sync::mpsc::channel();
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(SplitOpenedTransport {
            source: Box::new(ChannelSource(receiver)),
            sink: Box::new(sink),
            metadata: BTreeMap::new(),
        });
        let mut device = Device::new(0);
        sender.send(connection).unwrap();
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert!(device.poll(&mut host));

        let notification = AttPdu::HandleValueNotification {
            attribute_handle: 7,
            attribute_value: vec![0x44],
        };
        sender.send(att_acl(0x123, notification.clone())).unwrap();
        sender
            .send(att_acl(
                0x123,
                AttPdu::ReadResponse {
                    attribute_value: vec![1, 2, 3],
                },
            ))
            .unwrap();
        let mut transport =
            ExternalAttTransport::new(&mut host, &mut device, 0x123, Duration::from_secs(1))
                .unwrap();
        let mut client = bumble_gatt::GattClient::new();
        assert_eq!(
            client.read_value(&mut transport, 1, false).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(transport.take_unsolicited(), vec![notification]);
        assert!(recorded
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|packet| matches!(packet, HciPacket::AclData(_))));
    }

    #[test]
    fn device_surfaces_and_answers_external_ltk_requests() {
        let address =
            Address::parse("C4:F2:17:1A:1D:BB", bumble::AddressType::RANDOM_DEVICE).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let sink = RecordingSink::default();
        let recorded = sink.clone();
        let mut host = ExternalHost::new(SplitOpenedTransport {
            source: Box::new(ChannelSource(receiver)),
            sink: Box::new(sink),
            metadata: BTreeMap::new(),
        });
        let mut device = Device::new(0);
        sender
            .send(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::ConnectionComplete {
                    status: 0,
                    connection_handle: 0x123,
                    role: 1,
                    peer_address_type: 1,
                    peer_address: address,
                    connection_interval: 24,
                    peripheral_latency: 0,
                    supervision_timeout: 42,
                    central_clock_accuracy: 0,
                },
            )))
            .unwrap();
        sender
            .send(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::LongTermKeyRequest {
                    connection_handle: 0x123,
                    random_number: [0x22; 8],
                    encryption_diversifier: 0x3344,
                },
            )))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        let request = loop {
            device.poll(&mut host);
            if let Some(request) = device.take_long_term_key_requests().pop() {
                break request;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "LTK request was not surfaced");
            assert_eq!(
                host.wait_for_activity(remaining).unwrap(),
                ExternalHostActivity::Packet
            );
        };
        assert_eq!(request.connection_handle, 0x123);
        assert_eq!(request.random_number, [0x22; 8]);
        assert_eq!(request.encryption_diversifier, 0x3344);
        assert!(device.reply_long_term_key_request(&mut host, 0x123, [0xA5; 16]));
        assert!(recorded.0.lock().unwrap().iter().any(|packet| matches!(
            packet,
            HciPacket::Command(Command::LeLongTermKeyRequestReply {
                connection_handle: 0x123,
                long_term_key,
            }) if long_term_key == &[0xA5; 16]
        )));
    }

    #[test]
    fn le_pairing_session_drives_secure_connections_and_stores_bonds() {
        use bumble::keys::{KeyStore, MemoryKeyStore};
        use bumble::AddressType;
        use bumble_controller::{Controller, LocalLink as ControllerLink};
        use bumble_host::pump;

        let central_address =
            Address::parse("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE).unwrap();
        let peripheral_address =
            Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap();
        let mut link = ControllerLink::new();
        let central = link.add_controller(Controller::new(
            "central",
            Address::parse("00:00:00:00:00:01", AddressType::PUBLIC_DEVICE).unwrap(),
        ));
        let peripheral = link.add_controller(Controller::new(
            "peripheral",
            Address::parse("00:00:00:00:00:02", AddressType::PUBLIC_DEVICE).unwrap(),
        ));
        link.handle_command(
            peripheral,
            Command::LeSetRandomAddress {
                random_address: peripheral_address.clone(),
            },
        );
        link.handle_command(
            peripheral,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 1,
            },
        );
        link.handle_command(
            central,
            Command::LeSetRandomAddress {
                random_address: central_address.clone(),
            },
        );
        link.handle_command(
            central,
            Command::LeCreateConnection {
                le_scan_interval: 16,
                le_scan_window: 16,
                initiator_filter_policy: 0,
                peer_address_type: 1,
                peer_address: peripheral_address.clone(),
                own_address_type: 1,
                connection_interval_min: 24,
                connection_interval_max: 40,
                max_latency: 0,
                supervision_timeout: 42,
                min_ce_length: 0,
                max_ce_length: 0,
            },
        );
        link.establish_connections();
        let mut devices = [Device::new(central), Device::new(peripheral)];
        pump(&mut link, &mut devices);
        let handles = [
            devices[0].connection_handle().unwrap(),
            devices[1].connection_handle().unwrap(),
        ];
        let config = || PairingConfig {
            mitm: false,
            ..PairingConfig::default()
        };
        let mut sessions = [
            LePairingSession::accept_all(&devices[0], handles[0], central_address, config())
                .unwrap(),
            LePairingSession::accept_all(&devices[1], handles[1], peripheral_address, config())
                .unwrap(),
        ];
        sessions[0].begin(&mut link, &mut devices[0]).unwrap();
        sessions[1].begin(&mut link, &mut devices[1]).unwrap();

        let mut completed: [Option<PairingKeys>; 2] = [None, None];
        for _ in 0..200 {
            for index in 0..2 {
                if let Some(keys) = sessions[index]
                    .drive_once(&mut link, &mut devices[index])
                    .unwrap()
                {
                    completed[index] = Some(keys);
                }
            }
            if completed.iter().all(Option::is_some) {
                break;
            }
            pump(&mut link, &mut devices);
        }

        let central_keys = completed[0].as_ref().expect("central pairing completed");
        let peripheral_keys = completed[1].as_ref().expect("peripheral pairing completed");
        assert_eq!(central_keys.ltk, peripheral_keys.ltk);
        assert!(devices[0].is_encrypted_on_handle(handles[0]));
        assert!(devices[1].is_encrypted_on_handle(handles[1]));
        let mut stores = [MemoryKeyStore::new(), MemoryKeyStore::new()];
        assert!(sessions[0].store_bond(&mut stores[0]).unwrap());
        assert!(sessions[1].store_bond(&mut stores[1]).unwrap());
        assert_eq!(stores[0].get_all().unwrap().len(), 1);
        assert_eq!(stores[1].get_all().unwrap().len(), 1);
    }
}
