use bumble_avrcp::{
    ApplicationSettingAttributeId as AttributeId, ApplicationSettingValue as Value, BatteryStatus,
    CapabilityId, CharacterSetId, Command, Direction, EventId, MediaAttributeId, PduId,
    PlayerApplicationSetting, Scope,
};

fn samples() -> Vec<(Command, &'static str)> {
    vec![
        (Command::GetPlayStatus, ""),
        (
            Command::GetCapabilities {
                capability_id: CapabilityId::COMPANY_ID,
            },
            "02",
        ),
        (Command::SetAbsoluteVolume { volume: 5 }, "05"),
        (
            Command::GetElementAttributes {
                identifier: 999,
                attribute_ids: vec![MediaAttributeId::ALBUM_NAME, MediaAttributeId::ARTIST_NAME],
            },
            "00000000000003e7020000000300000002",
        ),
        (
            Command::RegisterNotification {
                event_id: EventId::ADDRESSED_PLAYER_CHANGED,
                playback_interval: 123,
            },
            "0b0000007b",
        ),
        (
            Command::Search {
                character_set_id: CharacterSetId::UTF_8,
                search_string: "Bumble!".into(),
            },
            "006a000742756d626c6521",
        ),
        (
            Command::PlayItem {
                scope: Scope::MEDIA_PLAYER_LIST,
                uid: 0,
                uid_counter: 1,
            },
            "0000000000000000000001",
        ),
        (Command::ListPlayerApplicationSettingAttributes, ""),
        (
            Command::ListPlayerApplicationSettingValues {
                attribute: AttributeId::REPEAT_MODE,
            },
            "02",
        ),
        (
            Command::GetCurrentPlayerApplicationSettingValue {
                attributes: vec![AttributeId::REPEAT_MODE, AttributeId::SHUFFLE_ON_OFF],
            },
            "020203",
        ),
        (
            Command::SetPlayerApplicationSettingValue {
                settings: vec![PlayerApplicationSetting {
                    attribute: AttributeId::REPEAT_MODE,
                    value: Value::ALL_TRACK_REPEAT,
                }],
            },
            "010203",
        ),
        (
            Command::GetPlayerApplicationSettingAttributeText {
                attributes: vec![AttributeId::REPEAT_MODE, AttributeId::SHUFFLE_ON_OFF],
            },
            "020203",
        ),
        (
            Command::GetPlayerApplicationSettingValueText {
                attribute: AttributeId::REPEAT_MODE,
                values: vec![Value::ALL_TRACK_REPEAT, Value::GROUP_REPEAT],
            },
            "02020304",
        ),
        (
            Command::InformDisplayableCharacterSet {
                character_set_ids: vec![CharacterSetId::UTF_8],
            },
            "01006a",
        ),
        (
            Command::InformBatteryStatusOfCt {
                battery_status: BatteryStatus::NORMAL,
            },
            "00",
        ),
        (Command::SetAddressedPlayer { player_id: 1 }, "0001"),
        (Command::SetBrowsedPlayer { player_id: 1 }, "0001"),
        (
            Command::GetFolderItems {
                scope: Scope::NOW_PLAYING,
                start_item: 0,
                end_item: 1,
                attributes: vec![MediaAttributeId::ARTIST_NAME],
            },
            "0300000000000000010100000002",
        ),
        (
            Command::ChangePath {
                uid_counter: 1,
                direction: Direction::DOWN,
                folder_uid: 2,
            },
            "0001010000000000000002",
        ),
        (
            Command::GetItemAttributes {
                scope: Scope::NOW_PLAYING,
                uid: 0,
                uid_counter: 1,
                attributes: vec![MediaAttributeId::DEFAULT_COVER_ART],
            },
            "03000000000000000000010108000000",
        ),
        (
            Command::GetTotalNumberOfItems {
                scope: Scope::NOW_PLAYING,
            },
            "03",
        ),
        (
            Command::AddToNowPlaying {
                scope: Scope::NOW_PLAYING,
                uid: 0,
                uid_counter: 1,
            },
            "0300000000000000000001",
        ),
    ]
}

#[test]
fn all_upstream_commands_are_byte_pinned_and_round_trip() {
    for (command, expected) in samples() {
        let parameters = command.to_parameters().unwrap();
        assert_eq!(parameters, hex(expected), "{command:?}");
        assert_eq!(
            Command::from_parameters(command.pdu_id(), &parameters).unwrap(),
            command
        );
    }
}

#[test]
fn malformed_and_unknown_commands_are_safe() {
    assert!(Command::from_parameters(PduId::SEARCH, &[0, 0x6A, 0, 2, b'a']).is_err());
    assert!(Command::from_parameters(PduId::GET_PLAY_STATUS, &[1]).is_err());
    let unknown = Command::Unknown {
        pdu_id: PduId(0xFE),
        parameters: vec![1, 2, 3],
    };
    assert_eq!(
        Command::from_parameters(PduId(0xFE), &[1, 2, 3]).unwrap(),
        unknown
    );
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
