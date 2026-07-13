use bumble_hci::{
    fragment_l2cap_pdu, AclDataPacket, AclDataPacketAssembler, HCI_ACL_PB_CONTINUATION,
    HCI_ACL_PB_FIRST_FLUSHABLE, HCI_ACL_PB_FIRST_NON_FLUSHABLE,
};

fn l2cap(payload: &[u8]) -> Vec<u8> {
    let mut pdu = Vec::new();
    pdu.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    pdu.extend_from_slice(&0x0040u16.to_le_bytes());
    pdu.extend_from_slice(payload);
    pdu
}

#[test]
fn fragments_and_reassembles_at_controller_buffer_boundary() {
    let pdu = l2cap(&(0..70).collect::<Vec<_>>());
    let packets = fragment_l2cap_pdu(0x0123, 0, 27, &pdu, false).unwrap();
    assert_eq!(packets.len(), 3);
    assert_eq!(packets[0].pb_flag, HCI_ACL_PB_FIRST_NON_FLUSHABLE);
    assert_eq!(packets[0].data.len(), 27);
    assert_eq!(packets[1].pb_flag, HCI_ACL_PB_CONTINUATION);
    assert_eq!(packets[1].data.len(), 27);
    assert_eq!(packets[2].pb_flag, HCI_ACL_PB_CONTINUATION);
    assert_eq!(packets[2].data.len(), 20);
    assert!(packets
        .iter()
        .all(|packet| usize::from(packet.data_total_length) == packet.data.len()));

    let mut assembler = AclDataPacketAssembler::new();
    assert_eq!(assembler.feed(&packets[0]).unwrap(), None);
    assert!(assembler.is_assembling());
    assert_eq!(assembler.feed(&packets[1]).unwrap(), None);
    assert_eq!(assembler.feed(&packets[2]).unwrap(), Some(pdu));
    assert!(!assembler.is_assembling());
}

#[test]
fn supports_flushable_and_single_packet_pdus() {
    let pdu = l2cap(b"short");
    let packets = fragment_l2cap_pdu(1, 2, 64, &pdu, true).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].pb_flag, HCI_ACL_PB_FIRST_FLUSHABLE);
    assert_eq!(packets[0].bc_flag, 2);
    assert_eq!(
        AclDataPacketAssembler::new().feed(&packets[0]).unwrap(),
        Some(pdu)
    );
}

#[test]
fn rejects_malformed_sequences_and_recovers_after_overflow() {
    let pdu = l2cap(b"12345678");
    assert!(fragment_l2cap_pdu(1, 0, 0, &pdu, false).is_err());
    assert!(fragment_l2cap_pdu(1, 0, 10, &pdu[..5], false).is_err());

    let mut assembler = AclDataPacketAssembler::new();
    let continuation = AclDataPacket {
        connection_handle: 1,
        pb_flag: HCI_ACL_PB_CONTINUATION,
        bc_flag: 0,
        data_total_length: 1,
        data: vec![1],
    };
    assert!(assembler.feed(&continuation).is_err());

    let mut packets = fragment_l2cap_pdu(1, 0, 6, &pdu, false).unwrap();
    assert_eq!(assembler.feed(&packets[0]).unwrap(), None);
    packets[1].connection_handle = 2;
    assert!(assembler.feed(&packets[1]).is_err());
    assert!(!assembler.is_assembling());

    let complete = fragment_l2cap_pdu(1, 0, 64, &pdu, false).unwrap().remove(0);
    assert_eq!(assembler.feed(&complete).unwrap(), Some(pdu));
}
