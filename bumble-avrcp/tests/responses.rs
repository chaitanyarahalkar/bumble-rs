use bumble_avrcp::{
    ApplicationSettingAttributeId as AttributeId, ApplicationSettingValue as Value, AttributeText,
    BrowseableItem, Capability, CapabilityId, CharacterSetId, Event, EventId, FolderType,
    MajorPlayerType, MediaAttribute, MediaAttributeId, MediaType, PduId, PlayStatus, Playable,
    PlayerApplicationSetting, PlayerFeatures, PlayerSubType, Response, StatusCode, ValueText,
};

fn samples() -> Vec<(Response, &'static str)> {
    let completed = StatusCode::OPERATION_COMPLETED;
    vec![
        (
            Response::GetPlayStatus {
                song_length: 1010,
                song_position: 13,
                play_status: PlayStatus::PAUSED,
            },
            "000003f20000000d02",
        ),
        (
            Response::GetCapabilities {
                capability_id: CapabilityId::EVENTS_SUPPORTED,
                capabilities: vec![
                    Capability::Event(EventId::ADDRESSED_PLAYER_CHANGED),
                    Capability::Event(EventId::BATT_STATUS_CHANGED),
                ],
            },
            "03020b06",
        ),
        (
            Response::RegisterNotification {
                event: Event::PlaybackPositionChanged {
                    playback_position: 38,
                },
            },
            "0500000026",
        ),
        (Response::SetAbsoluteVolume { volume: 99 }, "63"),
        (
            Response::GetElementAttributes {
                attributes: vec![MediaAttribute {
                    attribute_id: MediaAttributeId::ALBUM_NAME,
                    value: "White Album".into(),
                    character_set_id: CharacterSetId::UTF_8,
                }],
            },
            "0100000003006a000b576869746520416c62756d",
        ),
        (
            Response::ListPlayerApplicationSettingAttributes {
                attributes: vec![AttributeId::REPEAT_MODE, AttributeId::SHUFFLE_ON_OFF],
            },
            "020203",
        ),
        (
            Response::ListPlayerApplicationSettingValues {
                values: vec![Value::ALL_TRACK_REPEAT, Value::GROUP_REPEAT],
            },
            "020304",
        ),
        (
            Response::GetCurrentPlayerApplicationSettingValue {
                settings: vec![PlayerApplicationSetting {
                    attribute: AttributeId::REPEAT_MODE,
                    value: Value::ALL_TRACK_REPEAT,
                }],
            },
            "010203",
        ),
        (Response::SetPlayerApplicationSettingValue, ""),
        (
            Response::GetPlayerApplicationSettingAttributeText {
                entries: vec![AttributeText {
                    attribute: AttributeId::REPEAT_MODE,
                    character_set_id: CharacterSetId::UTF_8,
                    text: "Repeat".into(),
                }],
            },
            "0102006a06526570656174",
        ),
        (
            Response::GetPlayerApplicationSettingValueText {
                entries: vec![ValueText {
                    value: Value::ALL_TRACK_REPEAT,
                    character_set_id: CharacterSetId::UTF_8,
                    text: "All track repeat".into(),
                }],
            },
            "0103006a10416c6c20747261636b20726570656174",
        ),
        (Response::InformDisplayableCharacterSet, ""),
        (Response::InformBatteryStatusOfCt, ""),
        (
            Response::SetAddressedPlayer { status: completed },
            "04",
        ),
        (
            Response::SetBrowsedPlayer {
                status: completed,
                uid_counter: 1,
                number_of_items: 2,
                character_set_id: CharacterSetId::UTF_8,
                folder_names: vec!["folder1".into(), "folder2".into()],
            },
            "04000100000002006a020007666f6c646572310007666f6c64657232",
        ),
        (
            Response::GetFolderItems {
                status: completed,
                uid_counter: 1,
                items: vec![
                    BrowseableItem::MediaPlayer {
                        player_id: 1,
                        major_player_type: MajorPlayerType::AUDIO,
                        player_sub_type: PlayerSubType::AUDIO_BOOK,
                        play_status: PlayStatus::FWD_SEEK,
                        feature_bitmask: PlayerFeatures::ADD_TO_NOW_PLAYING,
                        character_set_id: CharacterSetId::UTF_8,
                        displayable_name: "Woo".into(),
                    },
                    BrowseableItem::Folder {
                        folder_uid: 1,
                        folder_type: FolderType::ALBUMS,
                        is_playable: Playable::PLAYABLE,
                        character_set_id: CharacterSetId::UTF_8,
                        displayable_name: "Album".into(),
                    },
                    BrowseableItem::MediaElement {
                        media_element_uid: 1,
                        media_type: MediaType::AUDIO,
                        character_set_id: CharacterSetId::UTF_8,
                        displayable_name: "Song".into(),
                        attributes: vec![],
                    },
                ],
            },
            "040001000301001f000101010000000300000000000000200000000000000000006a0003576f6f02001300000000000000010201006a0005416c62756d030012000000000000000100006a0004536f6e6700",
        ),
        (
            Response::ChangePath {
                status: completed,
                number_of_items: 2,
            },
            "0400000002",
        ),
        (
            Response::GetItemAttributes {
                status: completed,
                attributes: vec![MediaAttribute {
                    attribute_id: MediaAttributeId::GENRE,
                    character_set_id: CharacterSetId::UTF_8,
                    value: "uuddlrlrabab".into(),
                }],
            },
            "040100000006006a000c757564646c726c7261626162",
        ),
        (
            Response::GetTotalNumberOfItems {
                status: completed,
                uid_counter: 1,
                number_of_items: 2,
            },
            "04000100000002",
        ),
        (
            Response::Search {
                status: completed,
                uid_counter: 1,
                number_of_items: 2,
            },
            "04000100000002",
        ),
        (Response::PlayItem { status: completed }, "04"),
        (Response::AddToNowPlaying { status: completed }, "04"),
    ]
}

#[test]
fn all_upstream_responses_are_byte_pinned_and_round_trip() {
    for (response, expected) in samples() {
        let parameters = response.to_parameters().unwrap();
        assert_eq!(parameters, hex(expected), "{response:?}");
        assert_eq!(
            Response::from_parameters(response.pdu_id(), &parameters).unwrap(),
            response
        );
    }
}

#[test]
fn fallback_responses_and_unknown_items_are_lossless() {
    let rejected = Response::Rejected {
        pdu_id: PduId::SEARCH,
        status: StatusCode::INVALID_PARAMETER,
    };
    assert_eq!(rejected.to_parameters().unwrap(), vec![1]);
    let not_implemented = Response::NotImplemented {
        pdu_id: PduId(0xFE),
        parameters: vec![1, 2],
    };
    assert_eq!(not_implemented.to_parameters().unwrap(), vec![1, 2]);

    let response = Response::GetFolderItems {
        status: StatusCode::OPERATION_COMPLETED,
        uid_counter: 7,
        items: vec![BrowseableItem::Unknown {
            item_type: bumble_avrcp::BrowseableItemType(0xFE),
            data: vec![1, 2, 3],
        }],
    };
    let bytes = response.to_parameters().unwrap();
    assert_eq!(
        Response::from_parameters(PduId::GET_FOLDER_ITEMS, &bytes).unwrap(),
        response
    );
}

#[test]
fn malformed_nested_lengths_are_rejected() {
    let mut truncated = hex("04000100010100030102");
    assert!(Response::from_parameters(PduId::GET_FOLDER_ITEMS, &truncated).is_err());
    truncated = hex("0100000003006a00054869");
    assert!(Response::from_parameters(PduId::GET_ELEMENT_ATTRIBUTES, &truncated).is_err());
    assert!(Response::from_parameters(PduId::GET_PLAY_STATUS, &[0; 8]).is_err());
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
