use std::collections::BTreeSet;

use bumble_hfp::{
    AgConfiguration, AgEvent, AgFeatures, AgIndicator, AgIndicatorState, AgProtocol, AudioCodec,
    CallDirection, CallHoldOperation, CallInfo, CallMode, CallMultiParty, CallStatus,
    HfConfiguration, HfEvent, HfFeatures, HfIndicator, HfProtocol,
};

fn protocols() -> (HfProtocol, AgProtocol) {
    let hf_features =
        HfFeatures::CODEC_NEGOTIATION | HfFeatures::THREE_WAY_CALLING | HfFeatures::HF_INDICATORS;
    let ag_features =
        AgFeatures::CODEC_NEGOTIATION | AgFeatures::THREE_WAY_CALLING | AgFeatures::HF_INDICATORS;
    let indicators = BTreeSet::from([HfIndicator::EnhancedSafety, HfIndicator::BatteryLevel]);
    (
        HfProtocol::new(HfConfiguration {
            features: hf_features,
            indicators: indicators.iter().copied().collect(),
            codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
        }),
        AgProtocol::new(AgConfiguration {
            features: ag_features,
            indicators: vec![AgIndicatorState::call(), AgIndicatorState::service()],
            hf_indicators: indicators,
            call_hold_operations: BTreeSet::from([
                CallHoldOperation::ReleaseAllHeld,
                CallHoldOperation::HoldAllActive,
                CallHoldOperation::HoldAllExcept,
                CallHoldOperation::AddHeld,
            ]),
            codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
        }),
    )
}

fn exchange(hf: &mut HfProtocol, ag: &mut AgProtocol) {
    for _ in 0..64 {
        let mut progressed = false;
        for bytes in hf.drain_outgoing() {
            ag.feed(&bytes).unwrap();
            progressed = true;
        }
        for bytes in ag.drain_outgoing() {
            hf.feed(&bytes).unwrap();
            progressed = true;
        }
        if !progressed {
            return;
        }
    }
    panic!("HFP exchange did not quiesce");
}

fn complete_slc(hf: &mut HfProtocol, ag: &mut AgProtocol) {
    hf.start_slc().unwrap();
    exchange(hf, ag);
    assert!(hf.slc_complete && ag.slc_complete);
}

#[test]
fn call_control_current_calls_and_hf_indicators() {
    let (mut hf, mut ag) = protocols();
    complete_slc(&mut hf, &mut ag);

    let answer_id = hf.answer().unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::Answer]);
    assert_eq!(hf.take_completed_commands()[0].id, answer_id);

    hf.dial("123456789").unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::Dial("123456789".into())]);
    hf.take_completed_commands();

    hf.hang_up().unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::HangUp]);
    hf.take_completed_commands();

    let call = CallInfo {
        index: 1,
        direction: CallDirection::MobileOriginated,
        status: CallStatus::Active,
        mode: CallMode::Voice,
        multi_party: CallMultiParty::NotInConference,
        number: Some("123456789".into()),
        number_type: Some(129),
    };
    ag.calls.push(call.clone());
    hf.hold_call(CallHoldOperation::HoldAllExcept, Some(1))
        .unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(
        ag.take_events(),
        [AgEvent::CallHold {
            operation: CallHoldOperation::HoldAllExcept,
            call_index: Some(1),
        }]
    );
    hf.take_completed_commands();

    hf.query_current_calls().unwrap();
    exchange(&mut hf, &mut ag);
    let result = hf.take_completed_commands().pop().unwrap();
    assert_eq!(HfProtocol::parse_current_calls(&result).unwrap(), [call]);

    hf.report_hf_indicator(HfIndicator::BatteryLevel, 87)
        .unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(
        ag.take_events(),
        [AgEvent::HfIndicatorChanged {
            indicator: HfIndicator::BatteryLevel,
            value: 87,
        }]
    );
    hf.take_completed_commands();

    hf.execute_command("AT+BVRA=1", bumble_hfp::ResponseExpectation::None)
        .unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::VoiceRecognition(1)]);
    hf.take_completed_commands();

    hf.execute_command("AT+VGS=12", bumble_hfp::ResponseExpectation::None)
        .unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::SpeakerVolume(12)]);
    hf.take_completed_commands();

    hf.execute_command("AT+BCC", bumble_hfp::ResponseExpectation::None)
        .unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::CodecConnectionRequest]);
}

#[test]
fn unsolicited_indicators_volume_ring_caller_id_and_codec_negotiation() {
    let (mut hf, mut ag) = protocols();
    complete_slc(&mut hf, &mut ag);

    ag.update_ag_indicator(AgIndicator::Call, 1).unwrap();
    ag.send_ring();
    ag.set_speaker_volume(10);
    ag.set_microphone_volume(11);
    ag.send_caller_id("123456789", 129);
    ag.send_voice_recognition(1);
    ag.propose_codec(AudioCodec::Msbc).unwrap();
    for bytes in ag.drain_outgoing() {
        hf.feed(&bytes).unwrap();
    }
    assert_eq!(
        hf.take_events(),
        [
            HfEvent::AgIndicatorChanged {
                indicator: AgIndicator::Call,
                value: 1,
            },
            HfEvent::Ring,
            HfEvent::SpeakerVolume(10),
            HfEvent::MicrophoneVolume(11),
            HfEvent::CallerId {
                number: "123456789".into(),
                number_type: 129,
            },
            HfEvent::VoiceRecognition(1),
            HfEvent::CodecProposal(AudioCodec::Msbc),
        ]
    );

    hf.select_codec(AudioCodec::Msbc).unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::CodecSelected(AudioCodec::Msbc)]);
    assert_eq!(ag.active_codec, AudioCodec::Msbc);
    assert_eq!(hf.active_codec, AudioCodec::Msbc);
}
