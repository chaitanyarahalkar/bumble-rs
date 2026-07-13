use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_smp::{
    ClassicCtkdSession, ClassicCtkdState, PairingConfig, PairingRole, SmpPdu, SMP_BR_CID,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn connect_classic(
    link: &mut LocalLink,
    initiator: &mut Device,
    responder: &mut Device,
    initiator_address: &Address,
    responder_address: &Address,
) {
    initiator.connect_classic(link, responder_address.clone());
    initiator.poll(link);
    link.pump_classic();
    responder.poll(link);
    responder.accept_classic(link, initiator_address.clone());
    responder.poll(link);
    link.pump_classic();
    initiator.poll(link);
}

fn drive(link: &mut LocalLink, devices: &mut [Device; 2], sessions: &mut [ClassicCtkdSession; 2]) {
    for _ in 0..100 {
        let mut progress = false;
        for index in 0..2 {
            let handle = devices[index].classic_connection_handle().unwrap();
            for pdu in sessions[index].drain_outbound() {
                assert!(devices[index].send_l2cap_on_handle(
                    link,
                    handle,
                    SMP_BR_CID,
                    &pdu.to_bytes(),
                ));
                progress = true;
            }
        }
        pump(link, devices);
        for index in 0..2 {
            for payload in devices[index].take_l2cap(SMP_BR_CID) {
                sessions[index]
                    .process(SmpPdu::from_bytes(&payload).unwrap())
                    .unwrap();
                progress = true;
            }
        }
        if !progress {
            return;
        }
    }
    panic!("host-backed Classic CTKD did not quiesce");
}

#[test]
fn encrypted_classic_acl_runs_ctkd_over_fixed_l2cap_channel() {
    let initiator_address = address("11:11:11:11:11:11");
    let responder_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [Device::new(initiator_id), Device::new(responder_id)];
    let [initiator, responder] = &mut devices;
    connect_classic(
        &mut link,
        initiator,
        responder,
        &initiator_address,
        &responder_address,
    );
    assert!(devices[0].set_classic_encryption(&mut link, true));
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[1].poll(&mut link);
    assert!(devices[0].is_classic_encrypted());
    assert!(devices[1].is_classic_encrypted());

    let config = PairingConfig {
        ct2: true,
        ..PairingConfig::default()
    };
    let link_key = [0xC7; 16];
    let mut sessions = [
        ClassicCtkdSession::new(
            PairingRole::Initiator,
            config.clone(),
            initiator_address.clone(),
            responder_address.clone(),
            link_key,
            true,
            devices[0].is_classic_encrypted(),
        )
        .unwrap(),
        ClassicCtkdSession::new(
            PairingRole::Responder,
            config,
            initiator_address,
            responder_address,
            link_key,
            true,
            devices[1].is_classic_encrypted(),
        )
        .unwrap(),
    ];
    sessions[0].start().unwrap();
    drive(&mut link, &mut devices, &mut sessions);
    assert_eq!(sessions[0].state(), ClassicCtkdState::Complete);
    assert_eq!(sessions[1].state(), ClassicCtkdState::Complete);
    assert_eq!(sessions[0].outcome(), sessions[1].outcome());
    assert_eq!(
        sessions[0].pairing_keys().unwrap().link_key.unwrap().value,
        vec![0xC7; 16]
    );
}
