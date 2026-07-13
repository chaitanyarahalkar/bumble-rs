use bumble::Uuid;
use bumble_gatt::{GattClient, GattServer};
use bumble_hci::CodingFormat;
use bumble_profiles::bap::{
    AudioLocation, BasicAudioAnnouncement, BasicAudioBis, BasicAudioSubgroup,
    BroadcastAudioAnnouncement, CodecSpecificCapabilities, CodecSpecificConfiguration, ContextType,
    FrameDuration, SamplingFrequency, SupportedFrameDuration, SupportedSamplingFrequency,
    UnicastServerAdvertisingData,
};
use bumble_profiles::le_audio::{
    AudioActiveState, Metadata, MetadataEntry, MetadataTag, MetadataValue,
};
use bumble_profiles::pacs::{
    AudioContexts, PacCodecCapabilities, PacRecord, PublishedAudioCapabilitiesService,
    PublishedAudioCapabilitiesServiceProxy, AVAILABLE_AUDIO_CONTEXTS_CHARACTERISTIC,
};

fn capabilities() -> CodecSpecificCapabilities {
    CodecSpecificCapabilities {
        supported_sampling_frequencies: SupportedSamplingFrequency::FREQ_16000
            | SupportedSamplingFrequency::FREQ_48000,
        supported_frame_durations: SupportedFrameDuration::DURATION_7500_US_SUPPORTED
            | SupportedFrameDuration::DURATION_10000_US_SUPPORTED,
        supported_audio_channel_count: vec![1, 2],
        min_octets_per_codec_frame: 40,
        max_octets_per_codec_frame: 120,
        supported_max_codec_frames_per_sdu: 2,
    }
}

fn lc3_record() -> PacRecord {
    PacRecord {
        coding_format: CodingFormat {
            coding_format: 0x06,
            company_id: 0,
            vendor_specific_codec_id: 0,
        },
        codec_specific_capabilities: PacCodecCapabilities::Standard(capabilities()),
        metadata: Metadata::new(vec![MetadataEntry::new(
            MetadataTag::PROGRAM_INFO,
            b"LC3".to_vec(),
        )]),
    }
}

#[test]
fn metadata_matches_upstream_ltv_vector_and_typed_decoding() {
    let metadata = Metadata::new(vec![
        MetadataEntry::new(MetadataTag::PROGRAM_INFO, Vec::new()),
        MetadataEntry::new(MetadataTag::STREAMING_AUDIO_CONTEXTS, vec![0, 0]),
        MetadataEntry::new(MetadataTag::PREFERRED_AUDIO_CONTEXTS, vec![1, 2]),
    ]);
    let bytes = metadata.to_bytes().unwrap();
    assert_eq!(bytes, [1, 3, 3, 2, 0, 0, 3, 1, 1, 2]);
    assert_eq!(Metadata::from_bytes(&bytes).unwrap(), metadata);
    assert_eq!(
        metadata.entries[0].decode().unwrap(),
        MetadataValue::Text(String::new())
    );
    assert_eq!(
        metadata.entries[1].decode().unwrap(),
        MetadataValue::Context(ContextType::PROHIBITED)
    );
    assert_eq!(
        MetadataEntry::new(MetadataTag::AUDIO_ACTIVE_STATE, vec![1])
            .decode()
            .unwrap(),
        MetadataValue::AudioActiveState(AudioActiveState::AUDIO_DATA_TRANSMITTED)
    );
    assert!(Metadata::from_bytes(&[0]).is_err());
    assert!(Metadata::from_bytes(&[3, 1, 0]).is_err());
}

#[test]
fn bap_codec_ltv_models_and_unicast_advertising_are_byte_exact() {
    assert_eq!(
        SamplingFrequency::from_hz(48_000).unwrap(),
        SamplingFrequency::FREQ_48000
    );
    assert_eq!(SamplingFrequency::FREQ_44100.hz().unwrap(), 44_100);
    assert_eq!(
        FrameDuration::DURATION_7500_US.microseconds().unwrap(),
        7_500
    );
    assert_eq!(
        SupportedSamplingFrequency::from_hz(&[16_000, 48_000]).unwrap(),
        SupportedSamplingFrequency::FREQ_16000 | SupportedSamplingFrequency::FREQ_48000
    );

    let capabilities = capabilities();
    let bytes = capabilities.to_bytes().unwrap();
    assert_eq!(
        bytes,
        [3, 1, 0x84, 0x00, 2, 2, 3, 2, 3, 3, 5, 4, 40, 0, 120, 0, 2, 5, 2,]
    );
    assert_eq!(
        CodecSpecificCapabilities::from_bytes(&bytes).unwrap(),
        capabilities
    );

    let configuration = CodecSpecificConfiguration {
        sampling_frequency: Some(SamplingFrequency::FREQ_48000),
        frame_duration: Some(FrameDuration::DURATION_10000_US),
        audio_channel_allocation: Some(AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT),
        octets_per_codec_frame: Some(120),
        codec_frames_per_sdu: Some(1),
    };
    assert_eq!(
        CodecSpecificConfiguration::from_bytes(&configuration.to_bytes()).unwrap(),
        configuration
    );
    assert_eq!(
        UnicastServerAdvertisingData::default().to_bytes().unwrap(),
        [9, 0x16, 0x4E, 0x18, 1, 4, 0, 0, 0, 0]
    );
}

#[test]
fn broadcast_and_basic_audio_announcements_round_trip_exactly() {
    let broadcast = BroadcastAudioAnnouncement::new(123_456).unwrap();
    assert_eq!(broadcast.to_bytes().unwrap(), [0x40, 0xE2, 0x01]);
    assert_eq!(
        BroadcastAudioAnnouncement::from_bytes(&broadcast.to_bytes().unwrap()).unwrap(),
        broadcast
    );
    assert_eq!(
        broadcast.advertising_data().unwrap(),
        [6, 0x16, 0x52, 0x18, 0x40, 0xE2, 0x01]
    );

    let announcement = BasicAudioAnnouncement {
        presentation_delay: 40_000,
        subgroups: vec![BasicAudioSubgroup {
            codec_id: CodingFormat {
                coding_format: 0x06,
                company_id: 0,
                vendor_specific_codec_id: 0,
            },
            codec_specific_configuration: CodecSpecificConfiguration {
                sampling_frequency: Some(SamplingFrequency::FREQ_48000),
                frame_duration: Some(FrameDuration::DURATION_10000_US),
                octets_per_codec_frame: Some(100),
                ..CodecSpecificConfiguration::default()
            },
            metadata: Metadata::new(vec![
                MetadataEntry::new(MetadataTag::LANGUAGE, b"eng".to_vec()),
                MetadataEntry::new(MetadataTag::PROGRAM_INFO, b"Disco".to_vec()),
            ]),
            bis: vec![
                BasicAudioBis {
                    index: 0,
                    codec_specific_configuration: CodecSpecificConfiguration {
                        audio_channel_allocation: Some(AudioLocation::FRONT_LEFT),
                        ..CodecSpecificConfiguration::default()
                    },
                },
                BasicAudioBis {
                    index: 1,
                    codec_specific_configuration: CodecSpecificConfiguration {
                        audio_channel_allocation: Some(AudioLocation::FRONT_RIGHT),
                        ..CodecSpecificConfiguration::default()
                    },
                },
            ],
        }],
    };
    let bytes = announcement.to_bytes().unwrap();
    assert_eq!(
        BasicAudioAnnouncement::from_bytes(&bytes).unwrap(),
        announcement
    );
    let advertising = announcement.advertising_data().unwrap();
    assert_eq!(&advertising[1..4], [0x16, 0x51, 0x18]);
    assert_eq!(&advertising[4..], bytes);
    assert!(BasicAudioAnnouncement::from_bytes(&bytes[..bytes.len() - 1]).is_err());
    assert!(BroadcastAudioAnnouncement::new(0x0100_0000).is_err());
}

#[test]
fn pac_records_round_trip_standard_vendor_and_list_encodings() {
    let standard = lc3_record();
    let encoded = standard.to_bytes().unwrap();
    assert_eq!(&encoded[..5], [0x06, 0, 0, 0, 0]);
    let (decoded, consumed) = PacRecord::from_bytes(&encoded).unwrap();
    assert_eq!(consumed, encoded.len());
    assert_eq!(decoded, standard);

    let vendor = PacRecord {
        coding_format: CodingFormat {
            coding_format: 0xFF,
            company_id: 0x1234,
            vendor_specific_codec_id: 0x5678,
        },
        codec_specific_capabilities: PacCodecCapabilities::VendorSpecific(vec![9, 8, 7]),
        metadata: Metadata::default(),
    };
    let list = PacRecord::list_to_bytes(&[standard.clone(), vendor.clone()]).unwrap();
    assert_eq!(list[0], 2);
    assert_eq!(
        PacRecord::list_from_bytes(&list).unwrap(),
        [standard, vendor]
    );
    let vendor_raw = hex("ffe000ffff0000");
    let (vendor, consumed) = PacRecord::from_bytes(&vendor_raw).unwrap();
    assert_eq!(consumed, vendor_raw.len());
    assert_eq!(vendor.to_bytes().unwrap(), vendor_raw);
    assert!(PacRecord::list_from_bytes(&[1, 0]).is_err());
}

#[test]
fn published_audio_capabilities_live_proxy_reads_all_optional_values() {
    let supported = AudioContexts {
        sink: ContextType::MEDIA | ContextType::CONVERSATIONAL,
        source: ContextType::CONVERSATIONAL,
    };
    let available = AudioContexts {
        sink: ContextType::MEDIA,
        source: ContextType::PROHIBITED,
    };
    let vendor = PacRecord {
        coding_format: CodingFormat {
            coding_format: 0xFF,
            company_id: 1,
            vendor_specific_codec_id: 2,
        },
        codec_specific_capabilities: PacCodecCapabilities::VendorSpecific(vec![1, 2]),
        metadata: Metadata::default(),
    };
    let mut service = PublishedAudioCapabilitiesService::new(supported, available);
    service.sink_pac = vec![lc3_record()];
    service.sink_audio_locations = Some(AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT);
    service.source_pac = vec![vendor.clone()];
    service.source_audio_locations = Some(AudioLocation::FRONT_CENTER);
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let mut client = GattClient::new();
    let proxy = PublishedAudioCapabilitiesServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();

    assert_eq!(
        proxy
            .read_supported_contexts(&mut client, &mut server)
            .unwrap(),
        supported
    );
    assert_eq!(
        proxy
            .read_available_contexts(&mut client, &mut server)
            .unwrap(),
        available
    );
    assert_eq!(
        PublishedAudioCapabilitiesServiceProxy::read_pac(
            proxy.sink_pac.as_ref().unwrap(),
            &mut client,
            &mut server,
        )
        .unwrap(),
        [lc3_record()]
    );
    assert_eq!(
        PublishedAudioCapabilitiesServiceProxy::read_pac(
            proxy.source_pac.as_ref().unwrap(),
            &mut client,
            &mut server,
        )
        .unwrap(),
        [vendor]
    );
    assert_eq!(
        PublishedAudioCapabilitiesServiceProxy::read_audio_locations(
            proxy.sink_audio_locations.as_ref().unwrap(),
            &mut client,
            &mut server,
        )
        .unwrap(),
        AudioLocation::FRONT_LEFT | AudioLocation::FRONT_RIGHT
    );
    let descriptors = client
        .discover_descriptors(&mut server, &proxy.available_audio_contexts)
        .unwrap();
    assert_eq!(descriptors[0].uuid, Uuid::from_16_bits(0x2902));
    assert_eq!(
        server
            .handles_by_uuid(&Uuid::from_16_bits(AVAILABLE_AUDIO_CONTEXTS_CHARACTERISTIC))
            .len(),
        1
    );
}

#[test]
fn published_audio_capabilities_omits_empty_optional_characteristics() {
    let contexts = AudioContexts {
        sink: ContextType::MEDIA,
        source: ContextType::PROHIBITED,
    };
    let service = PublishedAudioCapabilitiesService::new(contexts, contexts);
    let mut server = GattServer::from_definitions(vec![service.definition().unwrap()]).unwrap();
    let mut client = GattClient::new();
    let proxy = PublishedAudioCapabilitiesServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert!(proxy.sink_pac.is_none());
    assert!(proxy.sink_audio_locations.is_none());
    assert!(proxy.source_pac.is_none());
    assert!(proxy.source_audio_locations.is_none());
}

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}
