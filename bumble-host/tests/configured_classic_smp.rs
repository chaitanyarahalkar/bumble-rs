use bumble::keys::{Key, KeyStore, MemoryKeyStore, PairingKeys};
use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{AclDataPacket, Command, Event, HciPacket, IsoDataPacket};
use bumble_host::{
    pump, ClassicPairingEvent, Device, DeviceConfiguration, DeviceEvent, HostTransport,
};
use bumble_smp::{ClassicCtkdState, ManagedPairingState, PairingFeatures, SmpPdu, SMP_BR_CID};

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn stored_link_key(peer_address: &Address, value: [u8; 16]) -> MemoryKeyStore {
    let mut store = MemoryKeyStore::new();
    store
        .update(
            &peer_address.to_string(false),
            PairingKeys {
                link_key: Some(Key {
                    value: value.to_vec(),
                    authenticated: true,
                    ..Key::default()
                }),
                link_key_type: Some(0x08),
                ..PairingKeys::default()
            },
        )
        .unwrap();
    store
}

#[test]
fn configured_encrypted_classic_connection_runs_managed_ctkd() {
    let initiator_address = public_address("11:11:11:11:11:11");
    let responder_address = public_address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let config = DeviceConfiguration {
        classic_enabled: true,
        classic_smp_enabled: true,
        classic_accept_any: true,
        ..DeviceConfiguration::default()
    };
    let mut devices = [
        Device::from_config(initiator_id, config.clone()).unwrap(),
        Device::from_config(responder_id, config).unwrap(),
    ];
    let link_key = [0xC7; 16];
    devices[0].set_key_store(stored_link_key(&responder_address, link_key));
    devices[1].set_key_store(stored_link_key(&initiator_address, link_key));
    devices[0].power_on(&mut link).unwrap();
    devices[1].power_on(&mut link).unwrap();
    pump(&mut link, &mut devices);

    devices[0].connect_classic(&mut link, responder_address.clone());
    pump(&mut link, &mut devices);
    assert!(devices[0].set_classic_encryption(&mut link, true));
    pump(&mut link, &mut devices);
    assert!(devices[0].is_classic_encrypted());
    assert!(devices[1].is_classic_encrypted());

    devices[0].pair_classic(&mut link).unwrap();
    pump(&mut link, &mut devices);
    let initiator_handle = devices[0].classic_connection_handle().unwrap();
    let responder_handle = devices[1].classic_connection_handle().unwrap();
    assert_eq!(
        devices[0].pairing_state(initiator_handle),
        Some(ManagedPairingState::ClassicCtkd(ClassicCtkdState::Complete))
    );
    assert_eq!(
        devices[1].pairing_state(responder_handle),
        Some(ManagedPairingState::ClassicCtkd(ClassicCtkdState::Complete))
    );
    let initiator_keys = devices[0].pairing_keys(initiator_handle).unwrap();
    let responder_keys = devices[1].pairing_keys(responder_handle).unwrap();
    assert_eq!(initiator_keys.ltk, responder_keys.ltk);
    assert_eq!(initiator_keys.link_key.as_ref().unwrap().value, link_key);
    assert!(devices[0]
        .take_device_events()
        .iter()
        .any(|event| matches!(event, DeviceEvent::PairingComplete { .. })));
    assert!(devices[1]
        .take_device_events()
        .iter()
        .any(|event| matches!(event, DeviceEvent::PairingComplete { .. })));

    let initiator_bond = devices[0].bond(&responder_address).unwrap().unwrap();
    let responder_bond = devices[1].bond(&initiator_address).unwrap().unwrap();
    assert_eq!(initiator_bond.ltk, responder_bond.ltk);
    assert_eq!(initiator_bond.link_key.unwrap().value, link_key);
    assert_eq!(responder_bond.link_key.unwrap().value, link_key);
    assert!(devices[0].take_pairing_errors().is_empty());
    assert!(devices[1].take_pairing_errors().is_empty());
    assert!(devices[0].take_key_store_errors().is_empty());
    assert!(devices[1].take_key_store_errors().is_empty());
}

#[test]
fn classic_ctkd_without_a_stored_link_key_returns_the_spec_failure() {
    let initiator_address = public_address("66:66:66:66:66:66");
    let responder_address = public_address("77:77:77:77:77:77");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [
        Device::new(initiator_id),
        Device::from_config(
            responder_id,
            DeviceConfiguration {
                classic_enabled: true,
                classic_smp_enabled: true,
                classic_accept_any: true,
                ..DeviceConfiguration::default()
            },
        )
        .unwrap(),
    ];
    devices[0].connect_classic(&mut link, responder_address);
    pump(&mut link, &mut devices);
    assert!(devices[0].set_classic_encryption(&mut link, true));
    pump(&mut link, &mut devices);

    let request = SmpPdu::PairingRequest(PairingFeatures {
        io_capability: 0x03,
        oob_data_flag: 0,
        auth_req: 0x09,
        maximum_encryption_key_size: 16,
        initiator_key_distribution: 0x07,
        responder_key_distribution: 0x07,
    });
    let handle = devices[0].classic_connection_handle().unwrap();
    assert!(devices[0].send_l2cap_on_handle(&mut link, handle, SMP_BR_CID, &request.to_bytes(),));
    pump(&mut link, &mut devices);
    let response = devices[0].take_l2cap(SMP_BR_CID);
    assert_eq!(
        SmpPdu::from_bytes(&response[0]).unwrap(),
        SmpPdu::PairingFailed { reason: 0x0E }
    );
    assert_eq!(devices[1].take_pairing_errors().len(), 1);
}

#[derive(Default)]
struct ScriptedTransport {
    events: Vec<HciPacket>,
    commands: Vec<Command>,
}

impl HostTransport for ScriptedTransport {
    fn handle_command(&mut self, _controller_id: usize, command: Command) {
        self.commands.push(command);
    }

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
    }

    fn send_synchronous_data(
        &mut self,
        _controller_id: usize,
        _connection_handle: u16,
        _packet_status: u8,
        _data: &[u8],
    ) -> bool {
        false
    }

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        std::mem::take(&mut self.events)
    }
}

#[test]
fn configured_link_key_provider_replies_and_persists_notifications() {
    let known_peer = public_address("33:33:33:33:33:33");
    let unknown_peer = public_address("44:44:44:44:44:44");
    let notified_peer = public_address("55:55:55:55:55:55");
    let known_key = [0xA1; 16];
    let notified_key = [0xB2; 16];
    let mut device = Device::from_config(0, DeviceConfiguration::default()).unwrap();
    device.set_key_store(stored_link_key(&known_peer, known_key));
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::LinkKeyRequest {
                bd_addr: known_peer.clone(),
            }),
            HciPacket::Event(Event::LinkKeyRequest {
                bd_addr: unknown_peer.clone(),
            }),
            HciPacket::Event(Event::LinkKeyNotification {
                bd_addr: notified_peer.clone(),
                link_key: notified_key,
                key_type: 0x08,
            }),
        ],
        commands: Vec::new(),
    };

    assert!(device.poll(&mut transport));
    assert_eq!(
        transport.commands,
        [
            Command::LinkKeyRequestReply {
                bd_addr: known_peer,
                link_key: known_key,
            },
            Command::LinkKeyRequestNegativeReply {
                bd_addr: unknown_peer,
            },
        ]
    );
    let stored = device.bond(&notified_peer).unwrap().unwrap();
    assert_eq!(stored.link_key.unwrap().value, notified_key);
    assert_eq!(stored.link_key_type, Some(0x08));
    assert!(device
        .take_device_events()
        .iter()
        .any(|event| matches!(event, DeviceEvent::KeyStoreUpdated)));
    assert!(matches!(
        device.take_classic_pairing_events().as_slice(),
        [
            ClassicPairingEvent::LinkKeyRequest { .. },
            ClassicPairingEvent::LinkKeyRequest { .. },
            ClassicPairingEvent::LinkKeyNotification { .. }
        ]
    ));
    assert!(device.take_key_store_errors().is_empty());
}
