use bumble_hci::HciPacket;
use bumble_transport::{
    BtSnooper, PacketSink, PacketSource, PcapSnooper, Result, SnoopDirection, Snooper,
    SnooperFormat, SnooperIoType, SnooperSpec, SnoopingTransport,
};
use std::collections::VecDeque;

#[test]
fn btsnoop_header_and_record_match_upstream_bytes() {
    let packet = [0x01, 0x03, 0x0C, 0x00];
    let mut snooper = BtSnooper::new(Vec::new()).unwrap();
    snooper
        .snoop_at_timestamp(
            &packet,
            SnoopDirection::HostToController,
            0x0102_0304_0506_0708,
        )
        .unwrap();

    let mut expected = b"btsnoop\0".to_vec();
    expected.extend_from_slice(&1u32.to_be_bytes());
    expected.extend_from_slice(&1002u32.to_be_bytes());
    expected.extend_from_slice(&4u32.to_be_bytes());
    expected.extend_from_slice(&4u32.to_be_bytes());
    expected.extend_from_slice(&0x10u32.to_be_bytes());
    expected.extend_from_slice(&0u32.to_be_bytes());
    expected.extend_from_slice(&0x0102_0304_0506_0708u64.to_be_bytes());
    expected.extend_from_slice(&packet);
    assert_eq!(snooper.into_inner(), expected);
}

#[test]
fn pcap_header_direction_pseudo_header_and_record_match_upstream_bytes() {
    let packet = [0x04, 0x0E, 0x01, 0x00];
    let mut snooper = PcapSnooper::new(Vec::new()).unwrap();
    snooper
        .snoop_at_timestamp(
            &packet,
            SnoopDirection::ControllerToHost,
            0x0102_0304,
            0x0005_0607,
        )
        .unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&0xA1B2_C3D4u32.to_le_bytes());
    expected.extend_from_slice(&2u16.to_le_bytes());
    expected.extend_from_slice(&4u16.to_le_bytes());
    expected.extend_from_slice(&0u32.to_le_bytes());
    expected.extend_from_slice(&0u32.to_le_bytes());
    expected.extend_from_slice(&65_535u32.to_le_bytes());
    expected.extend_from_slice(&201u32.to_le_bytes());
    expected.extend_from_slice(&0x0102_0304u32.to_le_bytes());
    expected.extend_from_slice(&0x0005_0607u32.to_le_bytes());
    expected.extend_from_slice(&8u32.to_le_bytes());
    expected.extend_from_slice(&8u32.to_le_bytes());
    expected.extend_from_slice(&1u32.to_be_bytes());
    expected.extend_from_slice(&packet);
    assert_eq!(snooper.into_inner(), expected);
}

#[derive(Default)]
struct MockTransport {
    inbound: VecDeque<HciPacket>,
    outbound: Vec<HciPacket>,
}

impl PacketSource for MockTransport {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        Ok(self.inbound.pop_front())
    }
}

impl PacketSink for MockTransport {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.outbound.push(packet.clone());
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSnooper(Vec<(Vec<u8>, SnoopDirection)>);

impl Snooper for RecordingSnooper {
    fn snoop(&mut self, packet: &[u8], direction: SnoopDirection) -> Result<()> {
        self.0.push((packet.to_vec(), direction));
        Ok(())
    }
}

#[test]
fn snooping_transport_records_both_directions_without_changing_packets() {
    let inbound = HciPacket::from_bytes(&[0x04, 0x0F, 0x04, 0x00, 0x01, 0x03, 0x0C]).unwrap();
    let outbound = HciPacket::from_bytes(&[0x01, 0x03, 0x0C, 0x00]).unwrap();
    let outbound_bytes = outbound.to_bytes();
    let mut inner = MockTransport::default();
    inner.inbound.push_back(inbound.clone());
    let mut transport = SnoopingTransport::new(inner, RecordingSnooper::default());

    assert_eq!(transport.read_packet().unwrap(), Some(inbound.clone()));
    transport.write_packet(&outbound).unwrap();
    let (inner, snooper) = transport.into_parts();
    assert_eq!(inner.outbound, [outbound]);
    assert_eq!(
        snooper.0,
        [
            (inbound.to_bytes(), SnoopDirection::ControllerToHost,),
            (outbound_bytes, SnoopDirection::HostToController),
        ]
    );
}

#[test]
fn snooper_specs_cover_upstream_file_and_pipe_forms() {
    let bt = SnooperSpec::parse("btsnoop:file:trace.btsnoop").unwrap();
    assert_eq!(bt.format, SnooperFormat::BtSnoop);
    assert_eq!(bt.io_type, SnooperIoType::File);
    assert_eq!(bt.path.to_string_lossy(), "trace.btsnoop");

    let pcap = SnooperSpec::parse("pcapsnoop:pipe:/tmp/bumble-extcap").unwrap();
    assert_eq!(pcap.format, SnooperFormat::Pcap);
    assert_eq!(pcap.io_type, SnooperIoType::Pipe);
    assert!(SnooperSpec::parse("btsnoop:pipe:/tmp/not-supported").is_err());
    assert!(SnooperSpec::parse("missing-io").is_err());
}
