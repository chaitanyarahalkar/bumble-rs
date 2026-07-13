use bumble_codecs::{AacAudioRtpPacket, BitReader, BitWriter};

#[test]
fn upstream_bit_reader_vectors() {
    assert!(BitReader::new(&[]).read(1).is_err());
    assert!(BitReader::new(b"hello").read(40).is_err());
    let mut reader = BitReader::new(&[0xFF]);
    assert_eq!(reader.read(1).unwrap(), 1);
    assert!(reader.read(10).is_err());

    let mut reader = BitReader::new(&[0x78]);
    let mut value = 0;
    for _ in 0..8 {
        value = (value << 1) | reader.read(1).unwrap();
    }
    assert_eq!(value, 0x78);

    let data: Vec<_> = (0..66 * 100).map(|value| value as u8).collect();
    let mut reader = BitReader::new(&data);
    for _ in 0..100 {
        for bits in 1..=32 {
            reader.read(bits).unwrap();
        }
    }
    assert_eq!(reader.bits_left(), 0);
}

#[test]
fn writer_round_trips_unaligned_chunks() {
    let chunks = [(1, 1), (3, 5), (8, 0xA5), (17, 0x1ABCD), (32, 0xDEADBEEF)];
    let mut writer = BitWriter::new();
    for (bits, value) in chunks {
        writer.write(value, bits).unwrap();
    }
    let data = writer.into_bytes();
    let mut reader = BitReader::new(&data);
    for (bits, value) in chunks {
        assert_eq!(reader.read(bits).unwrap(), value);
    }
}

#[test]
fn upstream_latm_fixture_converts_to_exact_adts() {
    let packet_data =
        hex("47fc0000b090800300202066000198000de120000000000000000000000000000000000000000000001c");
    let packet = AacAudioRtpPacket::from_bytes(&packet_data).unwrap();
    assert_eq!(
        packet.to_adts().unwrap(),
        hex("fff1508004fffc2066000198000de120000000000000000000000000000000000000000000001c")
    );
}

#[test]
fn simple_aac_round_trips_small_and_multi_length_payloads() {
    for size in [199, 255, 510, 700] {
        let payload: Vec<_> = (1..=size).map(|value| value as u8).collect();
        let packet = AacAudioRtpPacket::for_simple_aac(44_100, 2, payload.clone()).unwrap();
        let config = &packet
            .audio_mux_element
            .stream_mux_config
            .audio_specific_config;
        assert_eq!(config.sampling_frequency, 44_100);
        assert_eq!(config.channel_configuration, 2);
        let parsed = AacAudioRtpPacket::from_bytes(&packet.to_bytes().unwrap()).unwrap();
        assert_eq!(parsed.audio_mux_element.payload, payload);
        assert_eq!(
            parsed.audio_mux_element.stream_mux_config,
            packet.audio_mux_element.stream_mux_config
        );
    }
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap() as u8;
            let low = (pair[1] as char).to_digit(16).unwrap() as u8;
            (high << 4) | low
        })
        .collect()
}
