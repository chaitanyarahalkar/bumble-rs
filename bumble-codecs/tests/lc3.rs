use bumble_codecs::lc3::{Lc3Decoder, Lc3Encoder, Lc3Error, Lc3FrameDuration, Lc3StreamConfig};

fn config(channels: usize, frames: usize) -> Lc3StreamConfig {
    Lc3StreamConfig {
        sampling_frequency: 48_000,
        frame_duration: Lc3FrameDuration::TenMs,
        channels,
        octets_per_codec_frame: 100,
        codec_frames_per_sdu: frames,
    }
}

#[test]
fn stereo_multi_frame_sdu_round_trips_through_owned_workers() {
    let config = config(2, 2);
    let input = (0..config.pcm_samples_per_sdu())
        .map(|index| {
            let channel = index % config.channels;
            let frame_sample = index / config.channels;
            let period = if channel == 0 { 48 } else { 80 };
            (((frame_sample % period) as i32 * 800) - 16_000) as i16
        })
        .collect::<Vec<_>>();
    let encoder = Lc3Encoder::new(config).unwrap();
    let decoder = Lc3Decoder::new(config).unwrap();
    let sdu = encoder.encode_sdu(&input).unwrap();
    assert_eq!(sdu.len(), config.encoded_sdu_len());
    let decoded = decoder.decode_sdu(&sdu).unwrap();
    assert_eq!(decoded.len(), input.len());
    assert!(decoded.iter().any(|sample| *sample != 0));
    for channel in 0..config.channels {
        assert!(decoded
            .iter()
            .skip(channel)
            .step_by(config.channels)
            .any(|sample| *sample != 0));
    }
}

#[test]
fn validates_configuration_and_exact_buffer_shapes() {
    assert!(matches!(
        Lc3Encoder::new(config(0, 1)),
        Err(Lc3Error::InvalidConfiguration(_))
    ));
    let config = config(1, 1);
    let encoder = Lc3Encoder::new(config).unwrap();
    assert!(matches!(
        encoder.encode_sdu(&[0]),
        Err(Lc3Error::InvalidPcmLength { .. })
    ));
    let decoder = Lc3Decoder::new(config).unwrap();
    assert!(matches!(
        decoder.decode_sdu(&[0]),
        Err(Lc3Error::InvalidSduLength { .. })
    ));
}
