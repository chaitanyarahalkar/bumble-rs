use bumble_avctp::AVCTP_BROWSING_PSM;
use bumble_avrcp::sdp::*;
use bumble_sdp::service::{SdpClient, SdpServer};
use bumble_sdp::DataElement;

#[test]
fn controller_and_target_records_match_upstream_and_round_trip() {
    let controller = ControllerServiceSdpRecord {
        service_record_handle: 0x10001,
        avctp_version: ProfileVersion::V1_4,
        avrcp_version: ProfileVersion::V1_6,
        supported_features: ControllerFeatures::CATEGORY_1 | ControllerFeatures::SUPPORTS_BROWSING,
    };
    let target = TargetServiceSdpRecord {
        service_record_handle: 0x10002,
        avctp_version: ProfileVersion::V1_4,
        avrcp_version: ProfileVersion::V1_6,
        supported_features: TargetFeatures::CATEGORY_1 | TargetFeatures::SUPPORTS_BROWSING,
    };

    let controller_attributes = controller.to_service_attributes();
    let target_attributes = target.to_service_attributes();
    assert_eq!(controller_attributes.len(), 7);
    assert_eq!(target_attributes.len(), 7);
    assert_eq!(
        parse_controller_sdp_record(&controller_attributes),
        Some(controller)
    );
    assert_eq!(parse_target_sdp_record(&target_attributes), Some(target));

    for attributes in [&controller_attributes, &target_attributes] {
        let DataElement::Sequence(protocols) = attributes
            .iter()
            .find(|attribute| attribute.id == ADDITIONAL_PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID)
            .map(|attribute| &attribute.value)
            .unwrap()
        else {
            panic!("additional protocol descriptors must be a sequence");
        };
        let DataElement::Sequence(l2cap) = &protocols[0] else {
            panic!("L2CAP descriptor must be a sequence");
        };
        assert_eq!(
            l2cap[1],
            DataElement::unsigned_integer_16(AVCTP_BROWSING_PSM)
        );
    }
}

#[test]
fn records_are_found_through_the_sdp_client_server() {
    let controller = ControllerServiceSdpRecord {
        supported_features: ControllerFeatures::CATEGORY_1 | ControllerFeatures::SUPPORTS_BROWSING,
        ..ControllerServiceSdpRecord::new(0x10001)
    };
    let target = TargetServiceSdpRecord {
        supported_features: TargetFeatures::CATEGORY_1 | TargetFeatures::SUPPORTS_BROWSING,
        ..TargetServiceSdpRecord::new(0x10002)
    };
    let mut server = SdpServer::new(80);
    server.add_service(
        controller.service_record_handle,
        controller.to_service_attributes(),
    );
    server.add_service(target.service_record_handle, target.to_service_attributes());
    let mut client = SdpClient::new(server);
    assert_eq!(find_controller_records(&mut client).unwrap(), [controller]);
    assert_eq!(find_target_records(&mut client).unwrap(), [target]);
}

#[test]
fn malformed_role_or_protocol_records_are_rejected() {
    let mut controller = ControllerServiceSdpRecord::new(1).to_service_attributes();
    controller.retain(|attribute| attribute.id != PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID);
    assert_eq!(parse_controller_sdp_record(&controller), None);

    let target = ControllerServiceSdpRecord::new(2).to_service_attributes();
    assert_eq!(parse_target_sdp_record(&target), None);

    let defaults = TargetServiceSdpRecord::new(3);
    assert_eq!(defaults.avctp_version, ProfileVersion::new(1, 4));
    assert_eq!(defaults.avrcp_version, ProfileVersion::new(1, 6));
    assert_eq!(defaults.avrcp_version.major(), 1);
    assert_eq!(defaults.avrcp_version.minor(), 6);
    assert_eq!(defaults.to_service_attributes().len(), 6);
}
