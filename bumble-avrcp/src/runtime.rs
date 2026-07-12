use std::collections::{BTreeMap, BTreeSet};

use bumble_avc::{
    CommandType, Frame, FrameBody, OperationId, ResponseCode, StateFlag, SubunitType,
};
use bumble_avctp::Message;

use crate::{
    fragment_pdu, ApplicationSettingAttributeId, ApplicationSettingValue, Capability, CapabilityId,
    Command, Error, Event, EventId, PduAssembler, PduId, PlayStatus, PlayerApplicationSetting,
    Response, Result, StatusCode, AVRCP_PID, BLUETOOTH_SIG_COMPANY_ID,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandReply {
    pub response_code: ResponseCode,
    pub response: Response,
}

impl CommandReply {
    pub fn stable(response: Response) -> Self {
        Self {
            response_code: ResponseCode::IMPLEMENTED_OR_STABLE,
            response,
        }
    }

    pub fn accepted(response: Response) -> Self {
        Self {
            response_code: ResponseCode::ACCEPTED,
            response,
        }
    }

    pub fn interim(response: Response) -> Self {
        Self {
            response_code: ResponseCode::INTERIM,
            response,
        }
    }
}

/// Synchronous target behavior behind the transport-neutral runtime.
pub trait Delegate {
    fn handle_command(
        &mut self,
        command: &Command,
    ) -> core::result::Result<CommandReply, StatusCode>;

    fn on_key_event(
        &mut self,
        _operation_id: OperationId,
        _pressed: bool,
        _data: &[u8],
    ) -> core::result::Result<(), ResponseCode> {
        Ok(())
    }
}

/// Upstream-compatible in-memory target state for the command families Bumble
/// currently dispatches through its delegate.
#[derive(Clone, Debug)]
pub struct BasicDelegate {
    pub supported_events: Vec<EventId>,
    pub supported_company_ids: Vec<u32>,
    pub supported_player_app_settings:
        BTreeMap<ApplicationSettingAttributeId, Vec<ApplicationSettingValue>>,
    pub player_app_settings: BTreeMap<ApplicationSettingAttributeId, ApplicationSettingValue>,
    pub volume: u8,
    pub playback_status: PlayStatus,
    pub addressed_player_id: u16,
    pub uid_counter: u16,
    pub current_track_uid: u64,
    pub key_events: Vec<(OperationId, bool, Vec<u8>)>,
}

impl Default for BasicDelegate {
    fn default() -> Self {
        Self {
            supported_events: Vec::new(),
            supported_company_ids: vec![BLUETOOTH_SIG_COMPANY_ID],
            supported_player_app_settings: BTreeMap::new(),
            player_app_settings: BTreeMap::new(),
            volume: 0,
            playback_status: PlayStatus::STOPPED,
            addressed_player_id: 0,
            uid_counter: 0,
            current_track_uid: Event::NO_TRACK,
            key_events: Vec::new(),
        }
    }
}

impl BasicDelegate {
    fn current_event(&self, event_id: EventId) -> Option<Event> {
        match event_id {
            EventId::VOLUME_CHANGED => Some(Event::VolumeChanged {
                volume: self.volume,
            }),
            EventId::PLAYBACK_STATUS_CHANGED => Some(Event::PlaybackStatusChanged {
                play_status: self.playback_status,
            }),
            EventId::NOW_PLAYING_CONTENT_CHANGED => Some(Event::NowPlayingContentChanged),
            EventId::PLAYER_APPLICATION_SETTING_CHANGED => {
                Some(Event::PlayerApplicationSettingChanged {
                    settings: self
                        .player_app_settings
                        .iter()
                        .map(|(attribute, value)| PlayerApplicationSetting {
                            attribute: *attribute,
                            value: *value,
                        })
                        .collect(),
                })
            }
            EventId::AVAILABLE_PLAYERS_CHANGED => Some(Event::AvailablePlayersChanged),
            EventId::ADDRESSED_PLAYER_CHANGED => Some(Event::AddressedPlayerChanged {
                player_id: self.addressed_player_id,
                uid_counter: self.uid_counter,
            }),
            EventId::UIDS_CHANGED => Some(Event::UidsChanged {
                uid_counter: self.uid_counter,
            }),
            EventId::TRACK_CHANGED => Some(Event::TrackChanged {
                uid: self.current_track_uid,
            }),
            _ => None,
        }
    }
}

impl Delegate for BasicDelegate {
    fn handle_command(
        &mut self,
        command: &Command,
    ) -> core::result::Result<CommandReply, StatusCode> {
        match command {
            Command::GetCapabilities { capability_id } => {
                let capabilities = match *capability_id {
                    CapabilityId::EVENTS_SUPPORTED => self
                        .supported_events
                        .iter()
                        .copied()
                        .map(Capability::Event)
                        .collect(),
                    CapabilityId::COMPANY_ID => self
                        .supported_company_ids
                        .iter()
                        .copied()
                        .map(Capability::CompanyId)
                        .collect(),
                    _ => return Err(StatusCode::INVALID_PARAMETER),
                };
                Ok(CommandReply::stable(Response::GetCapabilities {
                    capability_id: *capability_id,
                    capabilities,
                }))
            }
            Command::SetAbsoluteVolume { volume } => {
                self.volume = *volume;
                Ok(CommandReply::accepted(Response::SetAbsoluteVolume {
                    volume: self.volume,
                }))
            }
            Command::GetPlayStatus => Ok(CommandReply::stable(Response::GetPlayStatus {
                song_length: Response::UNAVAILABLE,
                song_position: Response::UNAVAILABLE,
                play_status: self.playback_status,
            })),
            Command::ListPlayerApplicationSettingAttributes => Ok(CommandReply::stable(
                Response::ListPlayerApplicationSettingAttributes {
                    attributes: self.supported_player_app_settings.keys().copied().collect(),
                },
            )),
            Command::ListPlayerApplicationSettingValues { attribute } => Ok(CommandReply::stable(
                Response::ListPlayerApplicationSettingValues {
                    values: self
                        .supported_player_app_settings
                        .get(attribute)
                        .cloned()
                        .unwrap_or_default(),
                },
            )),
            Command::GetCurrentPlayerApplicationSettingValue { attributes } => {
                let mut settings = Vec::with_capacity(attributes.len());
                for attribute in attributes {
                    let Some(value) = self.player_app_settings.get(attribute).copied() else {
                        return Ok(CommandReply {
                            response_code: ResponseCode::NOT_IMPLEMENTED,
                            response: Response::NotImplemented {
                                pdu_id: command.pdu_id(),
                                parameters: Vec::new(),
                            },
                        });
                    };
                    settings.push(PlayerApplicationSetting {
                        attribute: *attribute,
                        value,
                    });
                }
                Ok(CommandReply::stable(
                    Response::GetCurrentPlayerApplicationSettingValue { settings },
                ))
            }
            Command::SetPlayerApplicationSettingValue { settings } => {
                for setting in settings {
                    self.player_app_settings
                        .insert(setting.attribute, setting.value);
                }
                Ok(CommandReply::stable(
                    Response::SetPlayerApplicationSettingValue,
                ))
            }
            Command::PlayItem { .. } => Ok(CommandReply::stable(Response::PlayItem {
                status: StatusCode::OPERATION_COMPLETED,
            })),
            Command::RegisterNotification { event_id, .. } => {
                if !self.supported_events.contains(event_id) {
                    return Ok(CommandReply {
                        response_code: ResponseCode::NOT_IMPLEMENTED,
                        response: Response::NotImplemented {
                            pdu_id: command.pdu_id(),
                            parameters: Vec::new(),
                        },
                    });
                }
                let event = self
                    .current_event(*event_id)
                    .ok_or(StatusCode::INVALID_PARAMETER)?;
                Ok(CommandReply::interim(Response::RegisterNotification {
                    event,
                }))
            }
            _ => Err(StatusCode::INVALID_PARAMETER),
        }
    }

    fn on_key_event(
        &mut self,
        operation_id: OperationId,
        pressed: bool,
        data: &[u8],
    ) -> core::result::Result<(), ResponseCode> {
        self.key_events.push((operation_id, pressed, data.to_vec()));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeEvent {
    Command {
        transaction_label: u8,
        command_type: CommandType,
        command: Command,
    },
    Response {
        transaction_label: u8,
        response_code: ResponseCode,
        response: Response,
    },
    PassThroughCommand {
        transaction_label: u8,
        operation_id: OperationId,
        pressed: bool,
        data: Vec<u8>,
    },
    PassThroughResponse {
        transaction_label: u8,
        response_code: ResponseCode,
        operation_id: OperationId,
        pressed: bool,
        data: Vec<u8>,
    },
    InvalidPid {
        transaction_label: u8,
    },
    Send(Message),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingKind {
    Vendor(PduId),
    PassThrough,
}

pub struct Runtime<D = BasicDelegate> {
    delegate: D,
    max_vendor_parameters: usize,
    free_labels: BTreeSet<u8>,
    pending: BTreeMap<u8, PendingKind>,
    command_assembler: PduAssembler,
    response_assembler: PduAssembler,
    incoming_command: Option<(u8, CommandType)>,
    incoming_response: Option<(u8, ResponseCode)>,
    notification_listeners: BTreeMap<EventId, u8>,
}

impl Runtime<BasicDelegate> {
    pub fn new(max_vendor_parameters: usize) -> Self {
        Self::with_delegate(BasicDelegate::default(), max_vendor_parameters)
    }
}

impl<D: Delegate> Runtime<D> {
    pub fn with_delegate(delegate: D, max_vendor_parameters: usize) -> Self {
        Self {
            delegate,
            max_vendor_parameters,
            free_labels: (0..16).collect(),
            pending: BTreeMap::new(),
            command_assembler: PduAssembler::new(),
            response_assembler: PduAssembler::new(),
            incoming_command: None,
            incoming_response: None,
            notification_listeners: BTreeMap::new(),
        }
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn begin_command(
        &mut self,
        command_type: CommandType,
        command: &Command,
    ) -> Result<Vec<Message>> {
        let transaction_label = self.reserve_label(PendingKind::Vendor(command.pdu_id()))?;
        match self.encode_command(transaction_label, command_type, command) {
            Ok(messages) => Ok(messages),
            Err(error) => {
                self.release_label(transaction_label);
                Err(error)
            }
        }
    }

    pub fn begin_pass_through(
        &mut self,
        operation_id: OperationId,
        pressed: bool,
        data: Vec<u8>,
    ) -> Result<Message> {
        let transaction_label = self.reserve_label(PendingKind::PassThrough)?;
        let frame = Frame::Command {
            command_type: CommandType::CONTROL,
            subunit_type: SubunitType::PANEL,
            subunit_id: 0,
            body: FrameBody::PassThrough {
                state: if pressed {
                    StateFlag::Pressed
                } else {
                    StateFlag::Released
                },
                operation_id,
                data,
            },
        };
        match frame.to_bytes() {
            Ok(payload) => Ok(Message::command(transaction_label, AVRCP_PID, payload)),
            Err(error) => {
                self.release_label(transaction_label);
                Err(error.into())
            }
        }
    }

    pub fn handle_message(&mut self, message: Message) -> Result<Vec<RuntimeEvent>> {
        if message.pid != AVRCP_PID {
            return Err(Error::WrongPid(message.pid));
        }
        if !message.is_command && message.ipid {
            self.release_label(message.transaction_label);
            return Ok(vec![RuntimeEvent::InvalidPid {
                transaction_label: message.transaction_label,
            }]);
        }
        let frame = Frame::from_bytes(&message.payload)?;
        if message.is_command {
            self.handle_command_frame(message.transaction_label, frame)
        } else {
            self.handle_response_frame(message.transaction_label, frame)
        }
    }

    pub fn notify(&mut self, event: Event) -> Result<Vec<Message>> {
        let event_id = event.event_id();
        let Some(transaction_label) = self.notification_listeners.get(&event_id).copied() else {
            return Ok(Vec::new());
        };
        let messages = self.encode_response(
            transaction_label,
            ResponseCode::CHANGED,
            &Response::RegisterNotification { event },
        )?;
        self.notification_listeners.remove(&event_id);
        Ok(messages)
    }

    fn reserve_label(&mut self, kind: PendingKind) -> Result<u8> {
        let label = self
            .free_labels
            .pop_first()
            .ok_or(Error::NoTransactionLabels)?;
        self.pending.insert(label, kind);
        Ok(label)
    }

    fn release_label(&mut self, label: u8) {
        self.pending.remove(&label);
        self.free_labels.insert(label);
    }

    fn encode_command(
        &self,
        transaction_label: u8,
        command_type: CommandType,
        command: &Command,
    ) -> Result<Vec<Message>> {
        let parameters = command.to_parameters()?;
        fragment_pdu(command.pdu_id(), &parameters, self.max_vendor_parameters)?
            .into_iter()
            .map(|pdu| {
                let frame = Frame::Command {
                    command_type,
                    subunit_type: SubunitType::PANEL,
                    subunit_id: 0,
                    body: FrameBody::VendorDependent {
                        company_id: BLUETOOTH_SIG_COMPANY_ID,
                        data: pdu.to_bytes()?,
                    },
                };
                Ok(Message::command(
                    transaction_label,
                    AVRCP_PID,
                    frame.to_bytes()?,
                ))
            })
            .collect()
    }

    fn encode_response(
        &self,
        transaction_label: u8,
        response_code: ResponseCode,
        response: &Response,
    ) -> Result<Vec<Message>> {
        let parameters = response.to_parameters()?;
        fragment_pdu(response.pdu_id(), &parameters, self.max_vendor_parameters)?
            .into_iter()
            .map(|pdu| {
                let frame = Frame::Response {
                    response_code,
                    subunit_type: SubunitType::PANEL,
                    subunit_id: 0,
                    body: FrameBody::VendorDependent {
                        company_id: BLUETOOTH_SIG_COMPANY_ID,
                        data: pdu.to_bytes()?,
                    },
                };
                Ok(Message::response(
                    transaction_label,
                    AVRCP_PID,
                    frame.to_bytes()?,
                ))
            })
            .collect()
    }

    fn handle_command_frame(
        &mut self,
        transaction_label: u8,
        frame: Frame,
    ) -> Result<Vec<RuntimeEvent>> {
        let Frame::Command {
            command_type,
            subunit_type,
            subunit_id,
            body,
        } = frame
        else {
            return Err(Error::WrongFrameKind);
        };
        if !((subunit_type == SubunitType::PANEL && subunit_id == 0)
            || (subunit_type == SubunitType::UNIT && subunit_id == 7))
        {
            return self.not_implemented_frame(transaction_label, subunit_type, subunit_id, body);
        }

        match body {
            FrameBody::VendorDependent { company_id, data } => {
                if company_id != BLUETOOTH_SIG_COMPANY_ID
                    || subunit_type != SubunitType::PANEL
                    || subunit_id != 0
                {
                    return Ok(Vec::new());
                }
                if let Some(state) = self.incoming_command {
                    if state != (transaction_label, command_type) {
                        self.command_assembler.reset();
                        self.incoming_command = None;
                        return Err(Error::InterleavedPdu);
                    }
                } else {
                    self.incoming_command = Some((transaction_label, command_type));
                }
                let complete = match self.command_assembler.push(&data) {
                    Ok(complete) => complete,
                    Err(error) => {
                        self.incoming_command = None;
                        return Err(error);
                    }
                };
                let Some((pdu_id, parameters)) = complete else {
                    return Ok(Vec::new());
                };
                self.incoming_command = None;
                let command = Command::from_parameters(pdu_id, &parameters)?;
                let mut events = vec![RuntimeEvent::Command {
                    transaction_label,
                    command_type,
                    command: command.clone(),
                }];
                let reply = match command_type {
                    CommandType::CONTROL | CommandType::STATUS | CommandType::NOTIFY => {
                        self.delegate.handle_command(&command)
                    }
                    _ => Err(StatusCode::INVALID_COMMAND),
                };
                let reply = match reply {
                    Ok(reply) => reply,
                    Err(status) => CommandReply {
                        response_code: ResponseCode::REJECTED,
                        response: Response::Rejected { pdu_id, status },
                    },
                };
                if reply.response.pdu_id() != pdu_id {
                    return Err(Error::MismatchedResponse {
                        expected: pdu_id,
                        actual: reply.response.pdu_id(),
                    });
                }
                if command_type == CommandType::NOTIFY
                    && reply.response_code == ResponseCode::INTERIM
                {
                    if let Command::RegisterNotification { event_id, .. } = command {
                        self.notification_listeners
                            .insert(event_id, transaction_label);
                    }
                }
                events.extend(
                    self.encode_response(transaction_label, reply.response_code, &reply.response)?
                        .into_iter()
                        .map(RuntimeEvent::Send),
                );
                Ok(events)
            }
            FrameBody::PassThrough {
                state,
                operation_id,
                data,
            } => {
                let pressed = state == StateFlag::Pressed;
                let response_code = match self.delegate.on_key_event(operation_id, pressed, &data) {
                    Ok(()) => ResponseCode::ACCEPTED,
                    Err(code) => code,
                };
                let response = Frame::Response {
                    response_code,
                    subunit_type,
                    subunit_id,
                    body: FrameBody::PassThrough {
                        state,
                        operation_id,
                        data: data.clone(),
                    },
                };
                Ok(vec![
                    RuntimeEvent::PassThroughCommand {
                        transaction_label,
                        operation_id,
                        pressed,
                        data,
                    },
                    RuntimeEvent::Send(Message::response(
                        transaction_label,
                        AVRCP_PID,
                        response.to_bytes()?,
                    )),
                ])
            }
            body => self.not_implemented_frame(transaction_label, subunit_type, subunit_id, body),
        }
    }

    fn not_implemented_frame(
        &self,
        transaction_label: u8,
        subunit_type: SubunitType,
        subunit_id: u16,
        body: FrameBody,
    ) -> Result<Vec<RuntimeEvent>> {
        let frame = Frame::Response {
            response_code: ResponseCode::NOT_IMPLEMENTED,
            subunit_type,
            subunit_id,
            body,
        };
        Ok(vec![RuntimeEvent::Send(Message::response(
            transaction_label,
            AVRCP_PID,
            frame.to_bytes()?,
        ))])
    }

    fn handle_response_frame(
        &mut self,
        transaction_label: u8,
        frame: Frame,
    ) -> Result<Vec<RuntimeEvent>> {
        let pending = self
            .pending
            .get(&transaction_label)
            .copied()
            .ok_or(Error::NotPending(transaction_label))?;
        let Frame::Response {
            response_code,
            subunit_type,
            subunit_id,
            body,
        } = frame
        else {
            return Err(Error::WrongFrameKind);
        };
        if response_code != ResponseCode::NOT_IMPLEMENTED
            && response_code != ResponseCode::ACCEPTED
            && response_code != ResponseCode::REJECTED
            && response_code != ResponseCode::IMPLEMENTED_OR_STABLE
            && response_code != ResponseCode::CHANGED
            && response_code != ResponseCode::INTERIM
        {
            return Err(Error::InvalidField("AVRCP response code"));
        }
        match (pending, body) {
            (
                PendingKind::PassThrough,
                FrameBody::PassThrough {
                    state,
                    operation_id,
                    data,
                },
            ) => {
                self.release_label(transaction_label);
                Ok(vec![RuntimeEvent::PassThroughResponse {
                    transaction_label,
                    response_code,
                    operation_id,
                    pressed: state == StateFlag::Pressed,
                    data,
                }])
            }
            (PendingKind::Vendor(expected), FrameBody::VendorDependent { company_id, data }) => {
                if company_id != BLUETOOTH_SIG_COMPANY_ID
                    || subunit_type != SubunitType::PANEL
                    || subunit_id != 0
                {
                    return Ok(Vec::new());
                }
                if let Some(state) = self.incoming_response {
                    if state != (transaction_label, response_code) {
                        self.response_assembler.reset();
                        self.incoming_response = None;
                        return Err(Error::InterleavedPdu);
                    }
                } else {
                    self.incoming_response = Some((transaction_label, response_code));
                }
                let complete = match self.response_assembler.push(&data) {
                    Ok(complete) => complete,
                    Err(error) => {
                        self.incoming_response = None;
                        return Err(error);
                    }
                };
                let Some((pdu_id, parameters)) = complete else {
                    return Ok(Vec::new());
                };
                self.incoming_response = None;
                if pdu_id != expected {
                    self.release_label(transaction_label);
                    return Err(Error::MismatchedResponse {
                        expected,
                        actual: pdu_id,
                    });
                }
                let response = if response_code == ResponseCode::REJECTED {
                    if parameters.len() != 1 {
                        return Err(Error::LengthMismatch {
                            declared: 1,
                            actual: parameters.len(),
                        });
                    }
                    Response::Rejected {
                        pdu_id,
                        status: StatusCode(parameters[0]),
                    }
                } else if response_code == ResponseCode::NOT_IMPLEMENTED {
                    Response::NotImplemented { pdu_id, parameters }
                } else {
                    Response::from_parameters(pdu_id, &parameters)?
                };
                if response_code != ResponseCode::INTERIM {
                    self.release_label(transaction_label);
                }
                Ok(vec![RuntimeEvent::Response {
                    transaction_label,
                    response_code,
                    response,
                }])
            }
            _ => Err(Error::WrongFrameKind),
        }
    }
}
