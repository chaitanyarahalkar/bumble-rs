use bumble_a2dp::*;
use bumble_avdtp::{MediaType, ServiceCapabilities};

#[test]
fn sbc_codec_specific_information_matches_upstream() {
    let expected = [0x3F, 0xFF, 0x02, 0x35];
    let info = SbcMediaCodecInformation::from_bytes(&expected).unwrap();
    assert_eq!(
        info.sampling_frequency,
        SbcSamplingFrequency::SF_44100 | SbcSamplingFrequency::SF_48000
    );
    assert_eq!(
        info.channel_mode,
        SbcChannelMode::MONO
            | SbcChannelMode::DUAL_CHANNEL
            | SbcChannelMode::STEREO
            | SbcChannelMode::JOINT_STEREO
    );
    assert_eq!(
        info.block_length,
        SbcBlockLength::BL_4 | SbcBlockLength::BL_8 | SbcBlockLength::BL_12 | SbcBlockLength::BL_16
    );
    assert_eq!(info.subbands, SbcSubbands::S_4 | SbcSubbands::S_8);
    assert_eq!(
        info.allocation_method,
        SbcAllocationMethod::SNR | SbcAllocationMethod::LOUDNESS
    );
    assert_eq!(info.minimum_bitpool_value, 2);
    assert_eq!(info.maximum_bitpool_value, 53);
    assert_eq!(info.to_bytes(), expected);
}

#[test]
fn aac_codec_specific_information_matches_upstream() {
    let expected = [0xF0, 0x01, 0x8C, 0x83, 0xE8, 0x00];
    let info = AacMediaCodecInformation::from_bytes(&expected).unwrap();
    assert_eq!(
        info.object_type,
        AacObjectType::MPEG_2_AAC_LC
            | AacObjectType::MPEG_4_AAC_LC
            | AacObjectType::MPEG_4_AAC_LTP
            | AacObjectType::MPEG_4_AAC_SCALABLE
    );
    assert_eq!(
        info.sampling_frequency,
        AacSamplingFrequency::SF_44100 | AacSamplingFrequency::SF_48000
    );
    assert_eq!(info.channels, AacChannels::MONO | AacChannels::STEREO);
    assert!(info.vbr);
    assert_eq!(info.bitrate, 256_000);
    assert_eq!(info.to_bytes().unwrap(), expected);
}

#[test]
fn opus_vendor_codec_and_avdtp_capability_match_upstream() {
    let info = OpusMediaCodecInformation::from_value(&[0x92]).unwrap();
    assert_eq!(info.channel_mode, OpusChannelMode::STEREO);
    assert_eq!(info.frame_size, OpusFrameSize::FS_20MS);
    assert_eq!(info.sampling_frequency, OpusSamplingFrequency::SF_48000);
    assert_eq!(info.value(), 0x92);

    let media = MediaCodecInformation::Opus(info);
    assert_eq!(media.to_bytes().unwrap(), [0xE0, 0, 0, 0, 1, 0, 0x92]);
    assert_eq!(
        MediaCodecInformation::parse(CodecType::NON_A2DP, &media.to_bytes().unwrap()).unwrap(),
        media
    );
    assert_eq!(
        media.to_avdtp_capability().unwrap(),
        ServiceCapabilities::MediaCodec {
            media_type: MediaType::AUDIO,
            media_codec_type: 0xFF,
            media_codec_information: vec![0xE0, 0, 0, 0, 1, 0, 0x92],
        }
    );
}

#[test]
fn truncated_and_out_of_range_codec_information_is_rejected() {
    assert!(SbcMediaCodecInformation::from_bytes(&[0; 3]).is_err());
    assert!(AacMediaCodecInformation::from_bytes(&[0; 5]).is_err());
    assert!(VendorSpecificMediaCodecInformation::from_bytes(&[0; 5]).is_err());
    let info = AacMediaCodecInformation {
        object_type: AacObjectType::MPEG_2_AAC_LC,
        sampling_frequency: AacSamplingFrequency::SF_48000,
        channels: AacChannels::STEREO,
        vbr: false,
        bitrate: 0x80_0000,
    };
    assert!(info.to_bytes().is_err());
}
