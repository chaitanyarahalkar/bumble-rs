use bumble_avrcp::{
    ApplicationSettingAttributeId as AttributeId, ApplicationSettingValue as Value, Event, EventId,
    PlayStatus, PlayerApplicationSetting,
};

fn samples() -> Vec<(Event, &'static str)> {
    vec![
        (Event::UidsChanged { uid_counter: 7 }, "0c0007"),
        (Event::TrackChanged { uid: 12356 }, "020000000000003044"),
        (Event::VolumeChanged { volume: 9 }, "0d09"),
        (
            Event::PlaybackStatusChanged {
                play_status: PlayStatus::PLAYING,
            },
            "0101",
        ),
        (
            Event::AddressedPlayerChanged {
                player_id: 9,
                uid_counter: 10,
            },
            "0b0009000a",
        ),
        (Event::AvailablePlayersChanged, "0a"),
        (
            Event::PlaybackPositionChanged {
                playback_position: 1314,
            },
            "0500000522",
        ),
        (Event::NowPlayingContentChanged, "09"),
        (
            Event::PlayerApplicationSettingChanged {
                settings: vec![PlayerApplicationSetting {
                    attribute: AttributeId::REPEAT_MODE,
                    value: Value::ALL_TRACK_REPEAT,
                }],
            },
            "08010203",
        ),
    ]
}

#[test]
fn all_upstream_events_are_byte_pinned_and_round_trip() {
    for (event, expected) in samples() {
        let bytes = event.to_bytes().unwrap();
        assert_eq!(bytes, hex(expected), "{event:?}");
        assert_eq!(Event::from_bytes(&bytes).unwrap(), event);
    }
}

#[test]
fn unregistered_events_are_lossless_and_malformed_known_events_fail() {
    let generic = Event::Unknown {
        event_id: EventId::TRACK_REACHED_END,
        data: vec![1, 2],
    };
    assert_eq!(
        Event::from_bytes(&generic.to_bytes().unwrap()).unwrap(),
        generic
    );
    assert!(Event::from_bytes(&[EventId::TRACK_CHANGED.0, 1]).is_err());
    assert!(Event::from_bytes(&[EventId::AVAILABLE_PLAYERS_CHANGED.0, 1]).is_err());
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
