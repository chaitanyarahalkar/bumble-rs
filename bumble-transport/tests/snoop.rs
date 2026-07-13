use bumble_hci::HciPacket;
use bumble_transport::{
    BtSnoopReader, BtSnooper, PacketSink, PacketSource, PcapSnooper, Result, SnoopDataLinkType,
    SnoopDirection, Snooper, SnooperFormat, SnooperIoType, SnooperSpec, SnoopingTransport,
};
use std::collections::VecDeque;
use std::io::Cursor;
use std::time::{Duration, UNIX_EPOCH};

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
fn btsnoop_reader_round_trips_h4_records_and_timestamps() {
    const DELTA: u64 = 0x00DC_DDB3_0F2F_8000;
    let command = [0x01, 0x03, 0x0C, 0x00];
    let event = [0x04, 0x0F, 0x04, 0x00, 0x01, 0x03, 0x0C];
    let mut snooper = BtSnooper::new(Vec::new()).unwrap();
    snooper
        .snoop_at_timestamp(
            &command,
            SnoopDirection::HostToController,
            DELTA + 1_234_567,
        )
        .unwrap();
    snooper
        .snoop_at_timestamp(&event, SnoopDirection::ControllerToHost, DELTA + 2_000_001)
        .unwrap();

    let mut reader = BtSnoopReader::new(Cursor::new(snooper.into_inner())).unwrap();
    assert_eq!(reader.version(), 1);
    assert_eq!(reader.data_link_type(), SnoopDataLinkType::H4);

    let first = reader.read_record().unwrap().unwrap();
    assert_eq!(first.direction(), SnoopDirection::HostToController);
    assert_eq!(first.packet_bytes(), command);
    assert_eq!(first.unix_timestamp_micros().unwrap(), 1_234_567);
    assert_eq!(
        first.system_time().unwrap(),
        UNIX_EPOCH + Duration::from_micros(1_234_567)
    );
    assert_eq!(first.packet().unwrap().unwrap().to_bytes(), command);

    let second = reader.read_record().unwrap().unwrap();
    assert_eq!(second.direction(), SnoopDirection::ControllerToHost);
    assert_eq!(second.packet().unwrap().unwrap().to_bytes(), event);
    assert_eq!(second.unix_timestamp_micros().unwrap(), 2_000_001);
    assert!(reader.read_record().unwrap().is_none());
}

#[test]
fn btsnoop_reader_reconstructs_h1_types_and_skips_truncated_packets() {
    let mut bytes = b"btsnoop\0".to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&(SnoopDataLinkType::H1 as u32).to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&0x00DC_DDB3_0F2F_8000u64.to_be_bytes());
    bytes.extend_from_slice(&[0x03, 0x0C, 0x00]);
    bytes.extend_from_slice(&5u32.to_be_bytes());
    bytes.extend_from_slice(&4u32.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&7u32.to_be_bytes());
    bytes.extend_from_slice(&0x00DC_DDB3_0F2F_8001u64.to_be_bytes());
    bytes.extend_from_slice(&[0x0E, 0x01, 0x00, 0xFF]);

    let mut reader = BtSnoopReader::new(Cursor::new(bytes)).unwrap();
    let command = reader.read_record().unwrap().unwrap();
    assert_eq!(command.packet_bytes(), [0x01, 0x03, 0x0C, 0x00]);
    assert!(matches!(
        command.packet().unwrap(),
        Some(HciPacket::Command(_))
    ));

    let truncated = reader.read_record().unwrap().unwrap();
    assert!(truncated.is_truncated());
    assert_eq!(truncated.cumulative_drops, 7);
    assert!(truncated.packet().unwrap().is_none());
    assert!(reader.read_record().unwrap().is_none());
}

#[test]
fn btsnoop_reader_rejects_bad_headers_and_unsupported_links() {
    let mut bad_id = [0u8; 16];
    bad_id[8..12].copy_from_slice(&1u32.to_be_bytes());
    bad_id[12..16].copy_from_slice(&(SnoopDataLinkType::H4 as u32).to_be_bytes());
    assert!(BtSnoopReader::new(Cursor::new(bad_id)).is_err());

    let mut h5 = b"btsnoop\0".to_vec();
    h5.extend_from_slice(&1u32.to_be_bytes());
    h5.extend_from_slice(&(SnoopDataLinkType::H5 as u32).to_be_bytes());
    assert!(BtSnoopReader::new(Cursor::new(h5)).is_err());
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
