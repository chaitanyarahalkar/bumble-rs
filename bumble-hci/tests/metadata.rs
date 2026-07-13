use bumble_hci::metadata::{
    codec_id_name, codec_transport_names, le_feature_names, specification_version_name,
    supported_command_names, VoiceSetting,
};

#[test]
fn symbolic_value_names_match_upstream() {
    assert_eq!(specification_version_name(13), Some("BLUETOOTH_CORE_5_4"));
    assert_eq!(specification_version_name(0xFE), None);
    assert_eq!(codec_id_name(6), Some("LC3"));
    assert_eq!(codec_id_name(0x80), None);
    assert_eq!(
        codec_transport_names(0b1101),
        vec!["BR_EDR_ACL", "LE_CIS", "LE_BIS"]
    );
}

#[test]
fn feature_and_command_bitmaps_decode_in_specification_order() {
    let mut features = [0u8; 10];
    features[0] = 1;
    features[6] = 1;
    features[7] = 0x80;
    features[9] = 1;
    assert_eq!(
        le_feature_names(&features),
        vec![
            "LE_ENCRYPTION",
            "CHANNEL_SOUNDING_TONE_QUALITY_INDICATION",
            "LL_EXTENDED_FEATURE_SET",
            "SHORTER_CONNECTION_INTERVALS",
        ]
    );

    let mut commands = [0u8; 64];
    commands[0] = 1;
    commands[5] = 0x80;
    commands[48] = 0x80;
    assert_eq!(
        supported_command_names(&commands),
        vec![
            "HCI_INQUIRY_COMMAND",
            "HCI_RESET_COMMAND",
            "HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND",
        ]
    );
}

#[test]
fn voice_setting_decodes_and_round_trips_defined_fields() {
    let setting = VoiceSetting::from_bits(0x0060);
    assert_eq!(setting.air_coding_format.name(), "CVSD");
    assert_eq!(setting.linear_pcm_bit_position, 0);
    assert_eq!(setting.input_sample_size.name(), "SIZE_16_BITS");
    assert_eq!(setting.input_data_format.name(), "TWOS_COMPLEMENT");
    assert_eq!(setting.input_coding_format.name(), "LINEAR");
    assert_eq!(setting.to_bits(), 0x0060);
}
