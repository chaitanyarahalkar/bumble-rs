use std::collections::BTreeSet;

use bumble::Uuid;
use bumble_hfp::sdp::{
    make_ag_sdp_record, make_hf_sdp_record, parse_ag_sdp_record, parse_hf_sdp_record,
    AgSdpFeatures, DiscoveredAgService, DiscoveredHfService, HfSdpFeatures, ProfileVersion,
    HANDSFREE_AUDIO_GATEWAY_SERVICE_UUID, HANDSFREE_SERVICE_UUID,
};
use bumble_hfp::{
    AgConfiguration, AgFeatures, AgIndicatorState, AudioCodec, CallHoldOperation, HfConfiguration,
    HfFeatures, HfIndicator,
};
use bumble_sdp::service::{AttributeId, SdpClient, SdpServer};

fn hf_configuration() -> HfConfiguration {
    HfConfiguration {
        features: HfFeatures::EC_NR
            | HfFeatures::THREE_WAY_CALLING
            | HfFeatures::CLI_PRESENTATION_CAPABILITY,
        indicators: vec![HfIndicator::BatteryLevel],
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    }
}

fn ag_configuration() -> AgConfiguration {
    AgConfiguration {
        features: AgFeatures::EC_NR
            | AgFeatures::THREE_WAY_CALLING
            | AgFeatures::IN_BAND_RING_TONE_CAPABILITY
            | AgFeatures::VOICE_RECOGNITION_FUNCTION,
        indicators: vec![AgIndicatorState::call()],
        hf_indicators: BTreeSet::from([HfIndicator::BatteryLevel]),
        call_hold_operations: BTreeSet::from([CallHoldOperation::HoldAllActive]),
        codecs: vec![AudioCodec::Cvsd, AudioCodec::Msbc],
    }
}

#[test]
fn hf_and_ag_records_round_trip_discovery_fields() {
    let hf_record = make_hf_sdp_record(1, 2, &hf_configuration(), ProfileVersion::V1_8);
    assert_eq!(
        parse_hf_sdp_record(&hf_record),
        Some(DiscoveredHfService {
            rfcomm_channel: 2,
            version: ProfileVersion::V1_8,
            features: HfSdpFeatures(
                HfSdpFeatures::EC_NR
                    | HfSdpFeatures::THREE_WAY_CALLING
                    | HfSdpFeatures::CLI_PRESENTATION_CAPABILITY
                    | HfSdpFeatures::WIDE_BAND_SPEECH
            ),
        })
    );
    assert!(parse_ag_sdp_record(&hf_record).is_none());

    let ag_record = make_ag_sdp_record(2, 3, &ag_configuration(), ProfileVersion::V1_9);
    assert_eq!(
        parse_ag_sdp_record(&ag_record),
        Some(DiscoveredAgService {
            rfcomm_channel: 3,
            version: ProfileVersion::V1_9,
            features: AgSdpFeatures(
                AgSdpFeatures::EC_NR
                    | AgSdpFeatures::THREE_WAY_CALLING
                    | AgSdpFeatures::IN_BAND_RING_TONE_CAPABILITY
                    | AgSdpFeatures::VOICE_RECOGNITION_FUNCTION
                    | AgSdpFeatures::WIDE_BAND_SPEECH
            ),
        })
    );
    assert!(parse_hf_sdp_record(&ag_record).is_none());
}

#[test]
fn records_are_discoverable_through_sdp_client_server() {
    let hf_record = make_hf_sdp_record(0x10000, 7, &hf_configuration(), ProfileVersion::V1_8);
    let ag_record = make_ag_sdp_record(0x10001, 8, &ag_configuration(), ProfileVersion::V1_8);
    let mut server = SdpServer::new(512);
    server.add_service(0x10000, hf_record);
    server.add_service(0x10001, ag_record);
    let mut client = SdpClient::new(server);

    let hf_results = client
        .service_search_attribute(
            &[Uuid::from_16_bits(HANDSFREE_SERVICE_UUID)],
            &[AttributeId::Range(0x0000, 0xffff)],
        )
        .unwrap();
    assert_eq!(hf_results.len(), 1);
    assert_eq!(
        parse_hf_sdp_record(&hf_results[0]).unwrap().rfcomm_channel,
        7
    );

    let ag_results = client
        .service_search_attribute(
            &[Uuid::from_16_bits(HANDSFREE_AUDIO_GATEWAY_SERVICE_UUID)],
            &[AttributeId::Range(0x0000, 0xffff)],
        )
        .unwrap();
    assert_eq!(ag_results.len(), 1);
    assert_eq!(
        parse_ag_sdp_record(&ag_results[0]).unwrap().rfcomm_channel,
        8
    );
}
