use bumble_att::AttPdu;
use bumble_gatt::{AccessContext, AttTransport, GattClient, GattError, GattServer};
use bumble_profiles::asha::{
    audio_type, codec, device_capabilities, opcode, AshaService, AshaServiceProxy, AUDIO_STATUS_OK,
};
use bumble_profiles::csip::{
    generate_rsi, k1, rsi_with_prand, s1, sef, sih, CoordinatedSetIdentificationProxy,
    CoordinatedSetIdentificationService, MemberLock, SirkType,
};
use bumble_profiles::Error;
use std::sync::{Arc, Mutex};

#[test]
fn asha_properties_advertising_and_audio_sink_match_upstream() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let service = AshaService::new(
        device_capabilities::IS_DUAL,
        (0u8..8).collect::<Vec<_>>(),
        0x0025,
    )
    .feature_map(3)
    .render_delay_milliseconds(4)
    .supported_codecs(5)
    .audio_sink(move |data| sink.lock().unwrap().push(data.to_vec()));

    assert_eq!(
        service.read_only_properties(),
        [1, 2, 0, 1, 2, 3, 4, 5, 6, 7, 3, 4, 0, 0, 0, 5, 0]
    );
    assert_eq!(
        service.advertising_data(),
        [9, 0x16, 0xF0, 0xFD, 1, 2, 0, 1, 2, 3]
    );
    service.receive_audio(&[1, 2, 3]);
    assert_eq!(*received.lock().unwrap(), [vec![1, 2, 3]]);
}

#[test]
fn asha_live_control_volume_status_and_psm_round_trip() {
    let service = AshaService::new(device_capabilities::IS_RIGHT, [8; 8], 0x0041);
    let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
    let handles = service.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = AshaServiceProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();

    assert_eq!(
        client
            .read_value(&mut server, proxy.psm_characteristic.handle, false)
            .unwrap(),
        0x0041u16.to_le_bytes()
    );
    client
        .write_value(
            &mut server,
            proxy.audio_control_point_characteristic.handle,
            vec![opcode::START, codec::G_722_16KHZ, audio_type::MEDIA, 99, 1],
            true,
        )
        .unwrap();
    let state = service.state().unwrap();
    assert_eq!(state.active_codec, Some(codec::G_722_16KHZ));
    assert_eq!(state.audio_type, Some(audio_type::MEDIA));
    assert_eq!(state.volume, Some(99));
    assert_eq!(state.other_state, Some(1));
    assert_eq!(state.starts, 1);
    assert_eq!(
        client
            .read_value(&mut server, proxy.audio_status_characteristic.handle, false,)
            .unwrap(),
        [AUDIO_STATUS_OK]
    );
    client
        .write_value(
            &mut server,
            proxy.volume_characteristic.handle,
            vec![37],
            false,
        )
        .unwrap();
    assert_eq!(service.state().unwrap().volume, Some(37));
    client
        .write_value(
            &mut server,
            handles.audio_control_point,
            vec![opcode::STATUS, 2],
            true,
        )
        .unwrap();
    assert_eq!(service.state().unwrap().peripheral_status, Some(2));
    client
        .write_value(
            &mut server,
            handles.audio_control_point,
            vec![opcode::STOP],
            true,
        )
        .unwrap();
    let state = service.state().unwrap();
    assert_eq!(state.active_codec, None);
    assert_eq!(state.volume, None);
    assert_eq!(state.stops, 1);
}

#[test]
fn csip_crypto_matches_upstream_specification_vectors() {
    let expected_salt = reversed_hex("6901983f18149e823c7d133a7d774572");
    assert_eq!(s1(&reversed(b"SIRKenc")), expected_salt.as_slice());

    let key = reversed_hex("676e1b9bd448696f061ec6223ce5ced9");
    assert_eq!(
        k1(&key, &expected_salt, &reversed(b"csis")).unwrap(),
        reversed_hex("5277453cc094d982b0e8ee532f2d1f8b").as_slice()
    );
    let sirk = reversed_hex("457d7d0921a1fd22cecd8c86dd72cccd");
    let prand: [u8; 3] = reversed_hex("69f563").try_into().unwrap();
    assert_eq!(
        sih(&sirk, &prand).unwrap(),
        reversed_hex("1948da").as_slice()
    );
    assert_eq!(
        sef(&key, &sirk).unwrap(),
        reversed_hex("170a3835e13524a07e2562d5f25fd346").as_slice()
    );
    let rsi = rsi_with_prand(&sirk, prand).unwrap();
    assert_eq!(&rsi[..3], reversed_hex("1948da"));
    assert_eq!(&rsi[3..], prand);
}

#[test]
fn csip_plaintext_and_encrypted_sirk_round_trip_over_encrypted_gatt() {
    let sirk: [u8; 16] = hex("2f62c8ae41867d1bb619e788a2605faa").try_into().unwrap();
    for sirk_type in [SirkType::Plaintext, SirkType::Encrypted] {
        let service = CoordinatedSetIdentificationService::new(&sirk, sirk_type)
            .unwrap()
            .coordinated_set_size(2)
            .set_member_lock(MemberLock::Unlocked)
            .set_member_rank(0)
            .encryption_key(move |_| Some(sirk));
        let mut server = GattServer::from_definitions(vec![service.definition()]).unwrap();
        service.bind(&mut server).unwrap();
        let mut transport = EncryptedTransport(&mut server);
        let mut client = GattClient::new();
        let proxy = CoordinatedSetIdentificationProxy::discover(&mut client, &mut transport)
            .unwrap()
            .unwrap();
        assert_eq!(
            proxy
                .read_set_identity_resolving_key(&mut client, &mut transport, Some(sirk))
                .unwrap(),
            (sirk_type, sirk)
        );
        assert_eq!(
            client
                .read_value(
                    &mut transport,
                    proxy.coordinated_set_size.as_ref().unwrap().handle,
                    false,
                )
                .unwrap(),
            [2]
        );
        assert_eq!(
            client
                .read_value(
                    &mut transport,
                    proxy.set_member_lock.as_ref().unwrap().handle,
                    false,
                )
                .unwrap(),
            [MemberLock::Unlocked as u8]
        );
        assert_eq!(
            client
                .read_value(
                    &mut transport,
                    proxy.set_member_rank.as_ref().unwrap().handle,
                    false,
                )
                .unwrap(),
            [0]
        );
    }
}

#[test]
fn csip_rejects_bad_lengths_and_generates_well_formed_rsi_advertising() {
    assert!(CoordinatedSetIdentificationService::new(&[0; 15], SirkType::Plaintext).is_err());
    assert!(sih(&[0; 16], &[0; 2]).is_err());
    assert!(sef(&[0; 15], &[0; 16]).is_err());

    let service = CoordinatedSetIdentificationService::new(&[7; 16], SirkType::Plaintext).unwrap();
    let advertising = service.advertising_data().unwrap();
    assert_eq!(advertising.len(), 8);
    assert_eq!(advertising[0..2], [7, 0x2E]);
    assert_eq!(advertising[7] & 0xC0, 0x40);
    let generated = generate_rsi(&[7; 16]).unwrap();
    assert_eq!(generated[5] & 0xC0, 0x40);
}

#[test]
fn csip_enforces_encryption_and_requires_key_material() {
    let plaintext =
        CoordinatedSetIdentificationService::new(&[1; 16], SirkType::Plaintext).unwrap();
    let mut server = GattServer::from_definitions(vec![plaintext.definition()]).unwrap();
    plaintext.bind(&mut server).unwrap();
    let mut client = GattClient::new();
    let proxy = CoordinatedSetIdentificationProxy::discover(&mut client, &mut server)
        .unwrap()
        .unwrap();
    assert!(matches!(
        proxy.read_set_identity_resolving_key(&mut client, &mut server, None),
        Err(Error::Gatt(GattError::Att {
            error_code: 0x0F,
            ..
        }))
    ));

    let encrypted =
        CoordinatedSetIdentificationService::new(&[2; 16], SirkType::Encrypted).unwrap();
    let mut server = GattServer::from_definitions(vec![encrypted.definition()]).unwrap();
    encrypted.bind(&mut server).unwrap();
    let mut transport = EncryptedTransport(&mut server);
    let mut client = GattClient::new();
    let proxy = CoordinatedSetIdentificationProxy::discover(&mut client, &mut transport)
        .unwrap()
        .unwrap();
    assert!(matches!(
        proxy.read_set_identity_resolving_key(&mut client, &mut transport, Some([2; 16])),
        Err(Error::Gatt(GattError::Att {
            error_code: 0x0E,
            ..
        }))
    ));
}

struct EncryptedTransport<'a>(&'a mut GattServer);

impl AttTransport for EncryptedTransport<'_> {
    fn request(&mut self, request: &AttPdu) -> AttPdu {
        self.0.on_request_with_context(
            request,
            AccessContext {
                bearer_id: 1,
                encrypted: true,
                authenticated: false,
                authorized: false,
            },
        )
    }
}

fn reversed(value: &[u8]) -> Vec<u8> {
    value.iter().rev().copied().collect()
}

fn reversed_hex(value: &str) -> Vec<u8> {
    let mut value = hex(value);
    value.reverse();
    value
}

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}
