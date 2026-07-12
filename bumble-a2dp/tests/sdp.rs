use bumble::Uuid;
use bumble_a2dp::sdp::*;
use bumble_sdp::service::{AttributeId, SdpClient, SdpServer};
use bumble_sdp::DataElement;

#[test]
fn source_and_sink_records_match_upstream_shape_and_parse() {
    let source = make_audio_source_sdp_record(0x10001, ProfileVersion::V1_3);
    let sink = make_audio_sink_sdp_record(0x10002, ProfileVersion::V1_3);
    assert_eq!(source.len(), 5);
    assert_eq!(sink.len(), 5);
    assert_eq!(
        parse_sdp_record(&source),
        Some(DiscoveredService {
            role: ServiceRole::Source,
            avdtp_version: ProfileVersion::V1_3,
            profile_version: ProfileVersion::V1_3,
        })
    );
    assert_eq!(
        parse_sdp_record(&sink),
        Some(DiscoveredService {
            role: ServiceRole::Sink,
            avdtp_version: ProfileVersion::V1_3,
            profile_version: ProfileVersion::V1_3,
        })
    );
}

#[test]
fn records_are_discoverable_through_sdp_client_server() {
    let mut server = SdpServer::new(128);
    server.add_service(
        0x10001,
        make_audio_source_sdp_record(0x10001, ProfileVersion::V1_3),
    );
    server.add_service(
        0x10002,
        make_audio_sink_sdp_record(0x10002, ProfileVersion::V1_3),
    );
    let mut client = SdpClient::new(server);
    for (uuid, role) in [
        (AUDIO_SOURCE_SERVICE_UUID, ServiceRole::Source),
        (AUDIO_SINK_SERVICE_UUID, ServiceRole::Sink),
    ] {
        let records = client
            .service_search_attribute(
                &[Uuid::from_16_bits(uuid)],
                &[AttributeId::Range(0x0000, 0xFFFF)],
            )
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(parse_sdp_record(&records[0]).unwrap().role, role);
    }
}

#[test]
fn wrong_role_or_protocol_shape_is_rejected() {
    let mut record = make_audio_sink_sdp_record(0x10002, ProfileVersion::V1_3);
    record.retain(|attribute| attribute.id != PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID);
    assert_eq!(parse_sdp_record(&record), None);

    let mut wrong_role = make_audio_sink_sdp_record(0x10002, ProfileVersion::V1_3);
    let service_class = wrong_role
        .iter_mut()
        .find(|attribute| attribute.id == SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID)
        .unwrap();
    service_class.value = DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(0x111E))]);
    assert_eq!(parse_sdp_record(&wrong_role), None);
}
