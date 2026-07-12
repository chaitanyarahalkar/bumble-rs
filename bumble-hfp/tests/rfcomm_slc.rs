use std::collections::BTreeSet;

use bumble_hfp::{
    AgConfiguration, AgFeatures, AgIndicatorState, AgProtocol, AudioCodec, CallHoldOperation,
    HfConfiguration, HfFeatures, HfIndicator, HfProtocol,
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

    for _ in 0..64 {
        for command in hf.drain_outgoing() {
            client.write(&mut client_manager, dlci, &command).unwrap();
        }
        drive_rfcomm(
            &mut client_manager,
            &mut client,
            &mut server_manager,
            &mut server,
        );
        for command in server.multiplexer_mut().take_rx(dlci) {
            ag.feed(&command).unwrap();
        }
        for response in ag.drain_outgoing() {
            server.write(&mut server_manager, dlci, &response).unwrap();
        }
        drive_rfcomm(
            &mut client_manager,
            &mut client,
            &mut server_manager,
            &mut server,
        );
        for response in client.multiplexer_mut().take_rx(dlci) {
            hf.feed(&response).unwrap();
        }
        if hf.slc_complete && ag.slc_complete {
            break;
        }
    }

    assert!(hf.slc_complete);
    assert!(ag.slc_complete);
    assert_eq!(hf.supported_ag_features, ag_features);
    assert_eq!(ag.supported_hf_features, hf_features);
    assert_eq!(hf.ag_indicators, ag.configuration().indicators);
    assert!(hf
        .hf_indicators
        .values()
        .all(|state| state.supported && state.enabled));
}
