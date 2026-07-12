use bumble_avctp::{L2capProtocol, Message, MessageAssembler, AVCTP_PSM};
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};

#[test]
fn upstream_single_and_fragmented_assembler_vectors() {
    let mut assembler = MessageAssembler::default();
    let payload = vec![1];
    assert_eq!(
        assembler.push(&[0x12, 0x11, 0x22, 1]).unwrap(),
        Some(Message::response(1, 0x1122, payload))
    );

    let mut assembler = MessageAssembler::default();
    assert_eq!(assembler.push(&[0x16, 3, 0x11, 0x22, 1]).unwrap(), None);
    assert_eq!(assembler.push(&[0x1A, 0x11, 0x22, 2]).unwrap(), None);
    assert_eq!(
        assembler.push(&[0x1E, 0x11, 0x22, 3]).unwrap(),
        Some(Message::response(1, 0x1122, vec![1, 2, 3]))
    );
}

#[test]
fn encoder_fragments_and_reassembles_large_payloads() {
    let message = Message::command(7, 0x110E, (0..100).collect());
    let pdus = message.encode_pdus(20).unwrap();
    assert!(pdus.len() > 2);
    let mut assembler = MessageAssembler::default();
    let mut complete = None;
    for pdu in pdus {
        if let Some(message) = assembler.push(&pdu).unwrap() {
            complete = Some(message);
        }
    }
    assert_eq!(complete, Some(message));
}

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

#[test]
fn commands_responses_and_ipid_run_over_classic_l2cap() {
    let mut controller_manager = ChannelManager::new();
    let mut target_manager = ChannelManager::new();
    target_manager
        .register_server(Some(AVCTP_PSM.into()), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    let controller_cid = controller_manager
        .connect(AVCTP_PSM.into(), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut controller_manager, &mut target_manager)
            + relay(&mut target_manager, &mut controller_manager);
        if count == 0 {
            break;
        }
    }
    let target_cid = target_manager.poll_accepted_channel().unwrap();
    let mut controller = L2capProtocol::new(controller_cid, &controller_manager).unwrap();
    let mut target = L2capProtocol::new(target_cid, &target_manager).unwrap();
    target.register_pid(0x110E);

    let command = Message::command(3, 0x110E, (0..80).collect());
    controller.send(&mut controller_manager, &command).unwrap();
    relay(&mut controller_manager, &mut target_manager);
    target.poll(&mut target_manager).unwrap();
    assert_eq!(target.take_messages(), [command]);

    controller
        .send(
            &mut controller_manager,
            &Message::command(4, 0x9999, vec![]),
        )
        .unwrap();
    relay(&mut controller_manager, &mut target_manager);
    target.poll(&mut target_manager).unwrap();
    relay(&mut target_manager, &mut controller_manager);
    controller.poll(&mut controller_manager).unwrap();
    assert_eq!(controller.take_messages(), [Message::ipid(4, 0x9999)]);
}

#[test]
fn malformed_remote_fragments_are_dropped() {
    let mut assembler = MessageAssembler::default();
    for pdu in [&[][..], &[0], &[0x01, 0, 0], &[0x04, 1, 0, 0]] {
        assert_eq!(assembler.push(pdu).unwrap(), None);
    }
}
