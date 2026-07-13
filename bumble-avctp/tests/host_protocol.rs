use bumble::{Address, AddressType};
use bumble_avctp::{DeviceProtocol, Message, AVCTP_PSM};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_l2cap::ClassicChannelSpec;

const ACCEPTED_PID: u16 = 0x110E;

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn connect_classic(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    initiator_address: &Address,
    responder_address: &Address,
) {
    devices[0].connect_classic(link, responder_address.clone());
    devices[0].poll(link);
    link.pump_classic();
    devices[1].poll(link);
    devices[1].accept_classic(link, initiator_address.clone());
    devices[1].poll(link);
    link.pump_classic();
    devices[0].poll(link);
}

fn drive(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    initiator: &mut DeviceProtocol,
    responder: &mut DeviceProtocol,
) {
    for _ in 0..64 {
        initiator.poll(link, &mut devices[0]).unwrap();
        responder.poll(link, &mut devices[1]).unwrap();
        pump(link, devices);
    }
}

#[test]
fn avctp_protocol_runs_over_device_managed_classic_channel() {
    let initiator_address = address("11:11:11:11:11:11");
    let responder_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let initiator_id = link.add_controller(Controller::new("A", initiator_address.clone()));
    let responder_id = link.add_controller(Controller::new("B", responder_address.clone()));
    let mut devices = [Device::new(initiator_id), Device::new(responder_id)];
    devices[1]
        .register_classic_channel_server(Some(u32::from(AVCTP_PSM)), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    connect_classic(
        &mut link,
        &mut devices,
        &initiator_address,
        &responder_address,
    );
    let initiator_handle = devices[0].classic_connection_handle().unwrap();
    let responder_handle = devices[1].classic_connection_handle().unwrap();
    let initiator_cid = devices[0]
        .connect_classic_channel(
            &mut link,
            initiator_handle,
            u32::from(AVCTP_PSM),
            ClassicChannelSpec { mtu: 48 },
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let responder_cid = devices[1]
        .take_accepted_classic_channels(responder_handle)
        .into_iter()
        .next()
        .unwrap();
    let mut initiator = DeviceProtocol::new(&devices[0], initiator_handle, initiator_cid).unwrap();
    let mut responder = DeviceProtocol::new(&devices[1], responder_handle, responder_cid).unwrap();
    responder.register_pid(ACCEPTED_PID);

    let fragmented = Message::command(4, ACCEPTED_PID, (0..100).collect());
    initiator
        .send(&mut link, &mut devices[0], &fragmented)
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert_eq!(responder.take_messages(), vec![fragmented]);

    let unknown = Message::command(9, 0x9999, vec![1, 2, 3]);
    initiator
        .send(&mut link, &mut devices[0], &unknown)
        .unwrap();
    drive(&mut link, &mut devices, &mut initiator, &mut responder);
    assert_eq!(
        initiator.take_messages(),
        vec![Message::ipid(unknown.transaction_label, unknown.pid)]
    );
    assert!(devices[0].take_classic_channel_errors().is_empty());
    assert!(devices[1].take_classic_channel_errors().is_empty());
}
