use crate::config::PandoraConfig;
use bumble::keys::{JsonKeyStore, KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType};
use bumble_hci::{Command, ReturnParameters};
use bumble_host::Device;
use bumble_transport::{open_split_transport, CommandResponse, ExternalHost, ExternalHostActivity};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tonic::Status;

pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const PROCEDURE_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum L2capChannelKind {
    Classic,
    LeCredit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct L2capChannelEntry {
    pub connection_handle: u16,
    pub source_cid: u16,
    pub psm: u32,
    pub kind: L2capChannelKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConnectionSecurity {
    pub authenticated: bool,
    pub secure_connections: bool,
    pub link_key_type: Option<u8>,
}

pub(crate) struct RuntimeState {
    pub host: ExternalHost,
    pub device: Device,
    pub config: PandoraConfig,
    pub public_address: Address,
    pub random_address: Address,
    pub key_store: Box<dyn KeyStore + Send>,
    pub connection_security: BTreeMap<u16, ConnectionSecurity>,
    pub waited_classic_connections: BTreeSet<u16>,
    pub classic_discoverable: bool,
    pub classic_connectable: bool,
    pub l2cap_channels: BTreeMap<Vec<u8>, L2capChannelEntry>,
    pub pending_l2cap_channels: Vec<L2capChannelEntry>,
    pub l2cap_classic_servers: BTreeSet<u32>,
    pub l2cap_le_servers: BTreeSet<u16>,
}

impl RuntimeState {
    fn open(
        config: PandoraConfig,
        rootcanal_port: u16,
        transport_override: Option<&str>,
    ) -> Result<Self, String> {
        let transport_name = config.transport(rootcanal_port, transport_override);
        let transport = open_split_transport(&transport_name).map_err(|error| error.to_string())?;
        let mut host = ExternalHost::new(transport);
        let mut device = Device::new(0);
        host.initialize_device(&mut device, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let random_address = Address::parse(&config.address, AddressType::RANDOM_DEVICE)
            .map_err(|error| error.to_string())?;
        successful_command(
            &mut host,
            Command::LeSetRandomAddress {
                random_address: random_address.clone(),
            },
            "setting Pandora random address",
        )?;
        let public_address = match host
            .send_command(Command::ReadBdAddr, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?
        {
            CommandResponse::Complete {
                return_parameters: ReturnParameters::ReadBdAddr { status: 0, bd_addr },
                ..
            } => bd_addr,
            response => return Err(format!("unexpected Read BD_ADDR response: {response:?}")),
        };
        let key_store = create_key_store(&config, &public_address);
        let mut state = Self {
            host,
            device,
            config,
            public_address,
            random_address,
            key_store,
            connection_security: BTreeMap::new(),
            waited_classic_connections: BTreeSet::new(),
            classic_discoverable: true,
            classic_connectable: true,
            l2cap_channels: BTreeMap::new(),
            pending_l2cap_channels: Vec::new(),
            l2cap_classic_servers: BTreeSet::new(),
            l2cap_le_servers: BTreeSet::new(),
        };
        state.apply_classic_scan_enable()?;
        Ok(state)
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.device = Device::new(0);
        self.host
            .initialize_device(&mut self.device, COMMAND_TIMEOUT)
            .map_err(|error| error.to_string())?;
        successful_command(
            &mut self.host,
            Command::LeSetRandomAddress {
                random_address: self.random_address.clone(),
            },
            "restoring Pandora random address",
        )?;
        self.waited_classic_connections.clear();
        self.connection_security.clear();
        self.l2cap_channels.clear();
        self.pending_l2cap_channels.clear();
        self.l2cap_classic_servers.clear();
        self.l2cap_le_servers.clear();
        self.apply_classic_scan_enable()
    }

    pub fn factory_reset(&mut self) -> Result<(), String> {
        self.key_store
            .delete_all()
            .map_err(|error| error.to_string())?;
        self.reset()
    }

    pub fn apply_classic_scan_enable(&mut self) -> Result<(), String> {
        let scan_enable =
            u8::from(self.classic_discoverable) | (u8::from(self.classic_connectable) << 1);
        successful_command(
            &mut self.host,
            Command::WriteScanEnable { scan_enable },
            "setting Classic discoverability/connectability",
        )
    }

    pub fn poll(&mut self, timeout: Duration) -> Result<bool, String> {
        self.device.poll(&mut self.host);
        match self
            .host
            .wait_for_device_activity(&mut self.device, timeout)
            .map_err(|error| error.to_string())?
        {
            ExternalHostActivity::Packet | ExternalHostActivity::Timeout => Ok(true),
            ExternalHostActivity::Ended => Ok(false),
        }
    }

    pub fn wait_for_le_connection(&mut self, peer: Option<&Address>) -> Result<u16, String> {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            self.device.poll(&mut self.host);
            let handle = peer
                .and_then(|peer| self.device.connection_handle_for_peer(peer))
                .or_else(|| {
                    peer.is_none()
                        .then(|| self.device.connection_handle())
                        .flatten()
                });
            if let Some(handle) = handle {
                return Ok(handle);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for LE connection".into());
            }
            if !self.poll(remaining.min(POLL_INTERVAL))? {
                return Err("transport ended while waiting for LE connection".into());
            }
        }
    }

    pub fn wait_for_classic_connection(&mut self, peer: &Address) -> Result<u16, String> {
        let deadline = Instant::now() + PROCEDURE_TIMEOUT;
        loop {
            self.device.poll(&mut self.host);
            if let Some(handle) = self.device.classic_connection_handle_for_peer(peer) {
                return Ok(handle);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("timed out waiting for Classic connection".into());
            }
            if !self.poll(remaining.min(POLL_INTERVAL))? {
                return Err("transport ended while waiting for Classic connection".into());
            }
        }
    }

    pub fn connection_exists(&self, handle: u16) -> bool {
        self.device.is_connected_on_handle(handle)
            || self.device.classic_connection(handle).is_some()
    }
}

fn create_key_store(config: &PandoraConfig, public_address: &Address) -> Box<dyn KeyStore + Send> {
    let Some(specification) = config.keystore.as_deref() else {
        return Box::new(MemoryKeyStore::new());
    };
    let (kind, filename) = specification
        .split_once(':')
        .map_or((specification, None), |(kind, filename)| {
            (kind, (!filename.is_empty()).then_some(filename))
        });
    if kind != "JsonKeyStore" {
        return Box::new(MemoryKeyStore::new());
    }
    let namespace = public_address.to_string(false);
    match filename {
        Some(filename) => Box::new(JsonKeyStore::new(Some(&namespace), filename)),
        None => Box::new(JsonKeyStore::with_default_path(Some(&namespace))),
    }
}

pub(crate) fn successful_command(
    host: &mut ExternalHost,
    command: Command,
    context: &str,
) -> Result<(), String> {
    let response = host
        .send_command(command, COMMAND_TIMEOUT)
        .map_err(|error| error.to_string())?;
    if response.status() == Some(0) {
        Ok(())
    } else {
        Err(format!(
            "{context} failed with status {:?}",
            response.status()
        ))
    }
}

#[derive(Clone)]
pub struct PandoraRuntime {
    pub(crate) state: Arc<Mutex<RuntimeState>>,
}

impl PandoraRuntime {
    pub fn open(
        config: PandoraConfig,
        rootcanal_port: u16,
        transport_override: Option<&str>,
    ) -> Result<Self, String> {
        Ok(Self {
            state: Arc::new(Mutex::new(RuntimeState::open(
                config,
                rootcanal_port,
                transport_override,
            )?)),
        })
    }

    pub(crate) async fn blocking<T, F>(&self, operation: F) -> Result<T, Status>
    where
        T: Send + 'static,
        F: FnOnce(&mut RuntimeState) -> Result<T, Status> + Send + 'static,
    {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            let mut state = state
                .lock()
                .map_err(|_| Status::internal("Pandora runtime lock poisoned"))?;
            operation(&mut state)
        })
        .await
        .map_err(|error| Status::internal(format!("Pandora worker failed: {error}")))?
    }
}

pub(crate) fn cookie(handle: u16) -> crate::proto::Connection {
    crate::proto::Connection {
        cookie: Some(prost_types::Any {
            type_url: String::new(),
            value: u32::from(handle).to_be_bytes().to_vec(),
        }),
    }
}

pub(crate) fn handle(connection: Option<crate::proto::Connection>) -> Result<u16, Status> {
    let value = connection
        .and_then(|connection| connection.cookie)
        .ok_or_else(|| Status::invalid_argument("connection cookie is required"))?
        .value;
    let bytes: [u8; 4] = value
        .try_into()
        .map_err(|_| Status::invalid_argument("connection cookie must contain four bytes"))?;
    u16::try_from(u32::from_be_bytes(bytes))
        .map_err(|_| Status::invalid_argument("connection cookie handle is out of range"))
}

pub(crate) fn address(bytes: Vec<u8>, address_type: AddressType) -> Result<Address, Status> {
    let bytes: [u8; 6] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("Bluetooth address must contain six bytes"))?;
    if bytes == [0; 6] || bytes == [0xFF; 6] {
        return Err(Status::invalid_argument("Bluetooth address is NIL or ANY"));
    }
    Ok(Address::from_bytes(bytes, address_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_cookies_match_upstream_big_endian_u32_encoding() {
        let connection = cookie(0x1234);
        assert_eq!(
            connection.cookie.as_ref().unwrap().value,
            vec![0, 0, 0x12, 0x34]
        );
        assert_eq!(handle(Some(connection)).unwrap(), 0x1234);
    }

    #[test]
    fn protocol_addresses_are_already_in_hci_little_endian_order() {
        let value = address(vec![1, 2, 3, 4, 5, 6], AddressType::PUBLIC_DEVICE).unwrap();
        assert_eq!(value.address_bytes(), &[1, 2, 3, 4, 5, 6]);
        assert!(address(vec![0; 6], AddressType::PUBLIC_DEVICE).is_err());
        assert!(address(vec![1; 5], AddressType::PUBLIC_DEVICE).is_err());
    }
}
