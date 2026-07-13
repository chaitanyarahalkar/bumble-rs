use bumble_gatt::{GattClient, GattServer};
use bumble_profiles::gmap::{
    BgrFeatures, BgsFeatures, GamingAudioService, GamingAudioServiceProxy, GmapRole, UggFeatures,
    UgtFeatures,
};
use bumble_profiles::le_audio::{Metadata, MetadataEntry, MetadataTag};
use bumble_profiles::pbp::{PublicBroadcastAnnouncement, PublicBroadcastFeatures};
use bumble_profiles::tmap::{
    Role, TelephonyAndMediaAudioService, TelephonyAndMediaAudioServiceProxy,
};

#[test]
fn tmap_combined_role_round_trips_through_live_proxy() {
    let role = Role::CALL_GATEWAY | Role::UNICAST_MEDIA_SENDER | Role::BROADCAST_MEDIA_RECEIVER;
    let service = TelephonyAndMediaAudioService::new(role);
    assert_eq!(service.definition().characteristics[0].value, [0x25, 0x00]);
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let mut client = GattClient::new();
    let proxy = TelephonyAndMediaAudioServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(proxy.read_role(&mut client, &mut server).unwrap(), role);
}

#[test]
fn gmap_all_role_features_match_upstream_live_service() {
    let role = GmapRole::UNICAST_GAME_GATEWAY
        | GmapRole::UNICAST_GAME_TERMINAL
        | GmapRole::BROADCAST_GAME_RECEIVER
        | GmapRole::BROADCAST_GAME_SENDER;
    let mut service = GamingAudioService::new(role);
    service.ugg_features = UggFeatures::UGG_MULTISINK;
    service.ugt_features = UgtFeatures::UGT_SOURCE;
    service.bgr_features = BgrFeatures::BGR_MULTISINK;
    service.bgs_features = BgsFeatures::BGS_96_KBPS;
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let mut client = GattClient::new();
    let proxy = GamingAudioServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(proxy.read_role(&mut client, &mut server).unwrap(), role);
    assert_eq!(
        proxy.read_ugg_features(&mut client, &mut server).unwrap(),
        Some(UggFeatures::UGG_MULTISINK)
    );
    assert_eq!(
        proxy.read_ugt_features(&mut client, &mut server).unwrap(),
        Some(UgtFeatures::UGT_SOURCE)
    );
    assert_eq!(
        proxy.read_bgr_features(&mut client, &mut server).unwrap(),
        Some(BgrFeatures::BGR_MULTISINK)
    );
    assert_eq!(
        proxy.read_bgs_features(&mut client, &mut server).unwrap(),
        Some(BgsFeatures::BGS_96_KBPS)
    );
}

#[test]
fn gmap_omits_features_for_roles_not_advertised() {
    let service = GamingAudioService::new(GmapRole::UNICAST_GAME_GATEWAY);
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let mut client = GattClient::new();
    let proxy = GamingAudioServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert_eq!(
        proxy.read_ugg_features(&mut client, &mut server).unwrap(),
        Some(UggFeatures::default())
    );
    assert!(proxy.ugt_features.is_none());
    assert!(proxy.bgs_features.is_none());
    assert!(proxy.bgr_features.is_none());
}

#[test]
fn public_broadcast_announcement_round_trips_and_advertises() {
    let announcement = PublicBroadcastAnnouncement {
        features: PublicBroadcastFeatures::ENCRYPTED
            | PublicBroadcastFeatures::HIGH_QUALITY_CONFIGURATION,
        metadata: Metadata::new(vec![MetadataEntry::new(
            MetadataTag::BROADCAST_NAME,
            b"Public".to_vec(),
        )]),
    };
    let value = announcement.to_bytes().unwrap();
    assert_eq!(value, [5, 8, 7, 0x0B, b'P', b'u', b'b', b'l', b'i', b'c']);
    assert_eq!(
        PublicBroadcastAnnouncement::from_bytes(&value).unwrap(),
        announcement
    );
    assert_eq!(
        announcement.advertising_data().unwrap(),
        [13, 0x16, 0x56, 0x18, 5, 8, 7, 0x0B, b'P', b'u', b'b', b'l', b'i', b'c',]
    );
    assert!(PublicBroadcastAnnouncement::from_bytes(&[1]).is_err());
    assert!(PublicBroadcastAnnouncement::from_bytes(&[1, 2, 0]).is_err());
}
