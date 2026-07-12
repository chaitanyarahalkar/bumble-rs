use bumble_l2cap::{ChannelManager, ClassicChannelSpec};
use bumble_rfcomm::l2cap::L2capMultiplexer;
use bumble_rfcomm::mux::{DlcState, MultiplexerState, Role};
use bumble_rfcomm::RFCOMM_PSM;

fn relay_l2cap(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

fn open_classic_channel() -> (ChannelManager, u16, ChannelManager, u16) {
    let mut client = ChannelManager::new();
    let mut server = ChannelManager::new();
    server
        .register_server(Some(RFCOMM_PSM.into()), ClassicChannelSpec { mtu: 512 })
        .unwrap();
    let client_cid = client
        .connect(RFCOMM_PSM.into(), ClassicChannelSpec { mtu: 512 })
        .unwrap();
    for _ in 0..32 {
        let count = relay_l2cap(&mut client, &mut server) + relay_l2cap(&mut server, &mut client);
        if count == 0 {
            break;
        }
    }
    let server_cid = server.poll_accepted_channel().unwrap();
    (client, client_cid, server, server_cid)
}

fn drive(
    client_manager: &mut ChannelManager,
    client: &mut L2capMultiplexer,
    server_manager: &mut ChannelManager,
    server: &mut L2capMultiplexer,
) {
    for _ in 0..128 {
        let mut count = relay_l2cap(client_manager, server_manager);
        count += relay_l2cap(server_manager, client_manager);
        count += client.poll(client_manager).unwrap();
        count += server.poll(server_manager).unwrap();
        if count == 0 {
            return;
        }
    }
    panic!("RFCOMM/L2CAP stack did not quiesce");
}

#[test]
fn rfcomm_session_and_credit_flow_run_over_classic_l2cap() {
    let (mut client_manager, client_cid, mut server_manager, server_cid) = open_classic_channel();
    let mut client = L2capMultiplexer::new(Role::Initiator, client_cid, &client_manager).unwrap();
    let mut server = L2capMultiplexer::new(Role::Responder, server_cid, &server_manager).unwrap();
    server.multiplexer_mut().listen(1, 32, 2);

    client.connect(&mut client_manager).unwrap();
    drive(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
    );
    assert_eq!(client.multiplexer().state(), MultiplexerState::Connected);
    assert_eq!(server.multiplexer().state(), MultiplexerState::Connected);

    client.open_dlc(&mut client_manager, 1, 32, 3).unwrap();
    drive(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
    );
    let dlci = 2;
    assert_eq!(
        client.multiplexer().dlc_state(dlci),
        Some(DlcState::Connected)
    );
    assert_eq!(
        server.multiplexer().dlc_state(dlci),
        Some(DlcState::Connected)
    );

    // More than two frames: the responder's two-credit offer forces a pause,
    // then its replenishment travels back through the same L2CAP channel.
    let payload: Vec<u8> = (0..100).collect();
    client.write(&mut client_manager, dlci, &payload).unwrap();
    drive(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
    );
    assert_eq!(server.multiplexer_mut().take_rx(dlci).concat(), payload);
    assert_eq!(client.multiplexer().dlc_pending_tx(dlci), Some(0));

    client.disconnect(&mut client_manager).unwrap();
    drive(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
    );
    assert_eq!(client.multiplexer().state(), MultiplexerState::Disconnected);
    assert_eq!(server.multiplexer().state(), MultiplexerState::Disconnected);
}
