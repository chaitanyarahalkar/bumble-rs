use std::collections::BTreeSet;

use bumble_hfp::{
    AgConfiguration, AgEvent, AgFeatures, AgIndicatorState, AgProtocol, AudioCodec,
    CallHoldOperation, CallLineIdentification, HfConfiguration, HfEvent, HfFeatures, HfIndicator,
    HfProtocol, VoiceRecognitionState,
};

fn protocols() -> (HfProtocol, AgProtocol) {
    let indicators = BTreeSet::from([HfIndicator::EnhancedSafety, HfIndicator::BatteryLevel]);
    (
        HfProtocol::new(HfConfiguration {
            features: HfFeatures::CODEC_NEGOTIATION
                | HfFeatures::THREE_WAY_CALLING
                | HfFeatures::HF_INDICATORS,
            indicators: indicators.iter().copied().collect(),
            codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
        }),
        AgProtocol::new(AgConfiguration {
            features: AgFeatures::CODEC_NEGOTIATION
                | AgFeatures::THREE_WAY_CALLING
                | AgFeatures::HF_INDICATORS
                | AgFeatures::EXTENDED_ERROR_RESULT_CODES,
            indicators: vec![AgIndicatorState::call(), AgIndicatorState::service()],
            hf_indicators: indicators,
            call_hold_operations: BTreeSet::from([CallHoldOperation::ReleaseAllHeld]),
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
fn public_call_and_audio_helpers_match_upstream_commands() {
    let (mut hf, mut ag) = protocols();
    complete_slc(&mut hf, &mut ag);

    hf.reject_incoming_call().unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::HangUp]);
    hf.take_completed_commands();

    hf.terminate_call().unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::HangUp]);
    hf.take_completed_commands();

    hf.setup_audio_connection().unwrap();
    exchange(&mut hf, &mut ag);
    assert_eq!(ag.take_events(), [AgEvent::CodecConnectionRequest]);
}

#[test]
fn extended_controls_typed_metadata_and_batched_commands() {
    let (mut hf, mut ag) = protocols();
    complete_slc(&mut hf, &mut ag);

    // Upstream accepts multiple AT commands in one RFCOMM write.
    ag.feed(b"AT+CMEE=1\rAT+CCWA=1\rAT+CLIP=1\rAT+BIA=0,1\r")
        .unwrap();
    assert!(ag.cme_error_enabled);
    assert!(ag.call_waiting_enabled);
    assert!(ag.cli_notification_enabled);
    assert!(!ag.configuration().indicators[0].enabled);
    assert!(ag.configuration().indicators[1].enabled);
    ag.drain_outgoing();

    // Extended errors are emitted only after CMEE has enabled them.
    ag.feed(b"AT+CHLD=9\r").unwrap();
    assert_eq!(ag.drain_outgoing(), [b"\r\n+CME ERROR: 4\r\n".to_vec()]);

    let mut caller = CallLineIdentification::new("123456789", 129);
    caller.subaddress = Some(String::new());
    caller.alpha = Some("Bumble".into());
    ag.send_cli_notification(&caller);
    ag.send_voice_recognition(VoiceRecognitionState::EnhancedReady);
    ag.set_inband_ringtone_enabled(false);
    for bytes in ag.drain_outgoing() {
        // +BSIR is optional and, like upstream, is safely ignored by the HF.
        hf.feed(&bytes).unwrap();
    }
    assert_eq!(
        hf.take_events(),
        [
            HfEvent::CallerId(caller),
            HfEvent::VoiceRecognition(VoiceRecognitionState::EnhancedReady),
        ]
    );
    assert!(!ag.inband_ringtone_enabled);
}
