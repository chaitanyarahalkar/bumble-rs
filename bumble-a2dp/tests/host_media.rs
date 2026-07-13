use bumble::{Address, AddressType};
use bumble_a2dp::transport::DeviceMediaTransport;
use bumble_avdtp::AVDTP_PSM;
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};
use bumble_l2cap::ClassicChannelSpec;
use bumble_rtp::MediaPacket;

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

#[test]
fn rtp_packets_cross_device_managed_avdtp_media_channel() {
    let source_address = address("11:11:11:11:11:11");
    let sink_address = address("22:22:22:22:22:22");
    let mut link = LocalLink::new();
    let source_id = link.add_controller(Controller::new("source", source_address.clone()));
    let sink_id = link.add_controller(Controller::new("sink", sink_address.clone()));
    let mut devices = [Device::new(source_id), Device::new(sink_id)];
    devices[1]
        .register_classic_channel_server(
            Some(u32::from(AVDTP_PSM)),
            ClassicChannelSpec { mtu: 128 },
        )
        .unwrap();
    devices[0].connect_classic(&mut link, sink_address);
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[1].poll(&mut link);
    devices[1].accept_classic(&mut link, source_address);
    devices[1].poll(&mut link);
    link.pump_classic();
    devices[0].poll(&mut link);
    let source_handle = devices[0].classic_connection_handle().unwrap();
    let sink_handle = devices[1].classic_connection_handle().unwrap();
    let source_cid = devices[0]
        .connect_classic_channel(
            &mut link,
            source_handle,
            u32::from(AVDTP_PSM),
            ClassicChannelSpec { mtu: 128 },
        )
        .unwrap();
    pump(&mut link, &mut devices);
    let sink_cid = devices[1]
        .take_accepted_classic_channels(sink_handle)
        .into_iter()
        .next()
        .unwrap();
    let source = DeviceMediaTransport::new(&devices[0], source_handle, source_cid).unwrap();
    let mut sink = DeviceMediaTransport::new(&devices[1], sink_handle, sink_cid).unwrap();
    let packet = MediaPacket::new(96, 7, 1024, 0x1234, vec![1, 2, 3, 4]);
    source.send(&mut link, &mut devices[0], &packet).unwrap();
    pump(&mut link, &mut devices);
    assert_eq!(sink.poll(&mut devices[1]).unwrap(), 1);
    assert_eq!(sink.take_packets(), [packet]);
}
