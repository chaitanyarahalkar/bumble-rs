use std::collections::BTreeSet;

use bumble_hfp::{
    AgConfiguration, AgEvent, AgFeatures, AgIndicator, AgIndicatorState, AgProtocol, AudioCodec,
    CallHoldOperation, HfConfiguration, HfEvent, HfFeatures, HfIndicator, HfProtocol,
};
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};
use bumble_rfcomm::l2cap::L2capMultiplexer;
use bumble_rfcomm::mux::{DlcState, Role};
use bumble_rfcomm::RFCOMM_PSM;

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) -> usize {
    let mut count = 0;
    while let Some(pdu) = left.poll_outbound() {
        right.process_pdu(pdu).unwrap();
        count += 1;
    }
    count
}

fn drive_rfcomm(
    client_manager: &mut ChannelManager,
    client: &mut L2capMultiplexer,
    server_manager: &mut ChannelManager,
    server: &mut L2capMultiplexer,
) {
    for _ in 0..128 {
        let mut count = relay(client_manager, server_manager);
        count += relay(server_manager, client_manager);
        count += client.poll(client_manager).unwrap();
        count += server.poll(server_manager).unwrap();
        if count == 0 {
            return;
        }
    }
    panic!("RFCOMM stack did not quiesce");
}

fn exchange_hfp(
    client_manager: &mut ChannelManager,
    client: &mut L2capMultiplexer,
    server_manager: &mut ChannelManager,
    server: &mut L2capMultiplexer,
    dlci: u8,
    hf: &mut HfProtocol,
    ag: &mut AgProtocol,
) {
    for _ in 0..64 {
        let mut progressed = false;
        for command in hf.drain_outgoing() {
            client.write(client_manager, dlci, &command).unwrap();
            progressed = true;
        }
        drive_rfcomm(client_manager, client, server_manager, server);
        for command in server.multiplexer_mut().take_rx(dlci) {
            ag.feed(&command).unwrap();
            progressed = true;
        }
        for response in ag.drain_outgoing() {
            server.write(server_manager, dlci, &response).unwrap();
            progressed = true;
        }
        drive_rfcomm(client_manager, client, server_manager, server);
        for response in client.multiplexer_mut().take_rx(dlci) {
            hf.feed(&response).unwrap();
            progressed = true;
        }
        if !progressed {
            return;
        }
    }
    panic!("HFP/RFCOMM exchange did not quiesce");
}

#[test]
fn hfp_slc_completes_over_rfcomm_and_classic_l2cap() {
    let mut client_manager = ChannelManager::new();
    let mut server_manager = ChannelManager::new();
    server_manager
        .register_server(Some(RFCOMM_PSM.into()), ClassicChannelSpec { mtu: 256 })
        .unwrap();
    let client_cid = client_manager
        .connect(RFCOMM_PSM.into(), ClassicChannelSpec { mtu: 256 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut client_manager, &mut server_manager)
            + relay(&mut server_manager, &mut client_manager);
        if count == 0 {
            break;
        }
    }
    let server_cid = server_manager.poll_accepted_channel().unwrap();
    let mut client = L2capMultiplexer::new(Role::Initiator, client_cid, &client_manager).unwrap();
    let mut server = L2capMultiplexer::new(Role::Responder, server_cid, &server_manager).unwrap();
    server.multiplexer_mut().listen(1, 96, 4);
    client.connect(&mut client_manager).unwrap();
    drive_rfcomm(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
    );
    client.open_dlc(&mut client_manager, 1, 96, 4).unwrap();
    drive_rfcomm(
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

    let hf_features =
        HfFeatures::CODEC_NEGOTIATION | HfFeatures::THREE_WAY_CALLING | HfFeatures::HF_INDICATORS;
    let ag_features =
        AgFeatures::CODEC_NEGOTIATION | AgFeatures::THREE_WAY_CALLING | AgFeatures::HF_INDICATORS;
    let indicators = BTreeSet::from([HfIndicator::EnhancedSafety, HfIndicator::BatteryLevel]);
    let mut hf = HfProtocol::new(HfConfiguration {
        features: hf_features,
        indicators: indicators.iter().copied().collect(),
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    });
    let mut ag = AgProtocol::new(AgConfiguration {
        features: ag_features,
        indicators: vec![AgIndicatorState::call(), AgIndicatorState::service()],
        hf_indicators: indicators,
        call_hold_operations: BTreeSet::from([
            CallHoldOperation::ReleaseAllHeld,
            CallHoldOperation::HoldAllActive,
            CallHoldOperation::AddHeld,
        ]),
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    });
    hf.start_slc().unwrap();

    exchange_hfp(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
        dlci,
        &mut hf,
        &mut ag,
    );

    assert!(hf.slc_complete);
    assert!(ag.slc_complete);
    assert_eq!(hf.supported_ag_features, ag_features);
    assert_eq!(ag.supported_hf_features, hf_features);
    assert_eq!(hf.ag_indicators, ag.configuration().indicators);
    assert!(hf
        .hf_indicators
        .values()
        .all(|state| state.supported && state.enabled));

    // Post-SLC commands and unsolicited events stay on the same live DLC.
    hf.answer().unwrap();
    exchange_hfp(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
        dlci,
        &mut hf,
        &mut ag,
    );
    assert_eq!(ag.take_events(), [AgEvent::Answer]);
    assert_eq!(hf.take_completed_commands().len(), 1);

    ag.update_ag_indicator(AgIndicator::Call, 1).unwrap();
    exchange_hfp(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
        dlci,
        &mut hf,
        &mut ag,
    );
    assert_eq!(
        hf.take_events(),
        [HfEvent::AgIndicatorChanged {
            indicator: AgIndicator::Call,
            value: 1,
        }]
    );

    ag.propose_codec(AudioCodec::Msbc).unwrap();
    exchange_hfp(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
        dlci,
        &mut hf,
        &mut ag,
    );
    assert_eq!(hf.take_events(), [HfEvent::CodecProposal(AudioCodec::Msbc)]);
    hf.select_codec(AudioCodec::Msbc).unwrap();
    exchange_hfp(
        &mut client_manager,
        &mut client,
        &mut server_manager,
        &mut server,
        dlci,
        &mut hf,
        &mut ag,
    );
    assert_eq!(ag.take_events(), [AgEvent::CodecSelected(AudioCodec::Msbc)]);
    assert_eq!(hf.active_codec, AudioCodec::Msbc);
    assert_eq!(ag.active_codec, AudioCodec::Msbc);
}
