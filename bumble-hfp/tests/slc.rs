use std::collections::BTreeSet;

use bumble_hfp::{
    AgConfiguration, AgFeatures, AgIndicator, AgIndicatorState, AgProtocol, AudioCodec,
    CallHoldOperation, HfConfiguration, HfFeatures, HfIndicator, HfProtocol,
};

fn drive(hf: &mut HfProtocol, ag: &mut AgProtocol) -> Vec<String> {
    let mut transcript = Vec::new();
    for _ in 0..64 {
        let mut progressed = false;
        for command in hf.drain_outgoing() {
            transcript.push(String::from_utf8(command[..command.len() - 1].to_vec()).unwrap());
            ag.feed(&command).unwrap();
            progressed = true;
        }
        for response in ag.drain_outgoing() {
            hf.feed(&response).unwrap();
            progressed = true;
        }
        if hf.slc_complete && ag.slc_complete {
            return transcript;
        }
        assert!(progressed, "SLC stalled before completion");
    }
    panic!("SLC did not complete");
}

#[test]
fn minimal_service_level_connection() {
    let mut hf = HfProtocol::new(HfConfiguration {
        features: HfFeatures::default(),
        indicators: vec![],
        codecs: vec![],
    });
    let mut ag = AgProtocol::new(AgConfiguration {
        features: AgFeatures::default(),
        indicators: vec![AgIndicatorState::call()],
        hf_indicators: BTreeSet::new(),
        call_hold_operations: BTreeSet::new(),
        codecs: vec![],
    });
    hf.start_slc().unwrap();
    let transcript = drive(&mut hf, &mut ag);

    assert_eq!(hf.supported_ag_features, AgFeatures::default());
    assert_eq!(ag.supported_hf_features, HfFeatures::default());
    assert_eq!(hf.ag_indicators, ag.configuration().indicators);
    assert!(ag.indicator_report_enabled);
    assert_eq!(
        transcript,
        ["AT+BRSF=0", "AT+CIND=?", "AT+CIND?", "AT+CMER=3,,,1"]
    );
}

#[test]
fn full_optional_service_level_connection() {
    let hf_features =
        HfFeatures::CODEC_NEGOTIATION | HfFeatures::THREE_WAY_CALLING | HfFeatures::HF_INDICATORS;
    let ag_features =
        AgFeatures::CODEC_NEGOTIATION | AgFeatures::THREE_WAY_CALLING | AgFeatures::HF_INDICATORS;
    let operations = BTreeSet::from([
        CallHoldOperation::ReleaseAllHeld,
        CallHoldOperation::ReleaseAllActive,
        CallHoldOperation::HoldAllActive,
        CallHoldOperation::AddHeld,
        CallHoldOperation::ConnectTwo,
    ]);
    let hf_indicators = BTreeSet::from([HfIndicator::EnhancedSafety, HfIndicator::BatteryLevel]);
    let mut hf = HfProtocol::new(HfConfiguration {
        features: hf_features,
        indicators: hf_indicators.iter().copied().collect(),
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    });
    let mut ag = AgProtocol::new(AgConfiguration {
        features: ag_features,
        indicators: vec![
            AgIndicatorState::call(),
            AgIndicatorState::service(),
            AgIndicatorState::call_setup(),
            AgIndicatorState::signal(),
        ],
        hf_indicators: hf_indicators.clone(),
        call_hold_operations: operations.clone(),
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    });
    hf.start_slc().unwrap();
    let transcript = drive(&mut hf, &mut ag);

    assert_eq!(hf.supported_ag_features, ag_features);
    assert_eq!(ag.supported_hf_features, hf_features);
    assert_eq!(
        ag.supported_audio_codecs,
        vec![AudioCodec::Cvsd, AudioCodec::Msbc]
    );
    assert_eq!(hf.ag_indicators, ag.configuration().indicators);
    assert_eq!(hf.supported_ag_call_hold_operations, operations);
    assert_eq!(ag.hf_indicators.len(), 2);
    assert!(hf
        .hf_indicators
        .values()
        .all(|state| state.supported && state.enabled));
    assert_eq!(
        transcript,
        [
            "AT+BRSF=386",
            "AT+BAC=1,2",
            "AT+CIND=?",
            "AT+CIND?",
            "AT+CMER=3,,,1",
            "AT+CHLD=?",
            "AT+BIND=1,2",
            "AT+BIND=?",
            "AT+BIND?",
        ]
    );
}

#[test]
fn default_ag_indicator_factories_match_upstream() {
    let cases = [
        (AgIndicatorState::call(), AgIndicator::Call, 0, 1),
        (AgIndicatorState::call_setup(), AgIndicator::CallSetup, 0, 3),
        (AgIndicatorState::call_held(), AgIndicator::CallHeld, 0, 2),
        (AgIndicatorState::service(), AgIndicator::Service, 0, 1),
        (AgIndicatorState::signal(), AgIndicator::Signal, 0, 5),
        // Upstream's public roam() factory currently selects CALL.
        (AgIndicatorState::roam(), AgIndicator::Call, 0, 1),
        (
            AgIndicatorState::battery_charge(),
            AgIndicator::BatteryCharge,
            0,
            5,
        ),
    ];

    for (state, indicator, minimum, maximum) in cases {
        assert_eq!(state.indicator, indicator);
        assert_eq!(state.current_status, 0);
        assert_eq!(state.supported_values.first().copied(), Some(minimum));
        assert_eq!(state.supported_values.last().copied(), Some(maximum));
        assert!(state.enabled);
    }
}
