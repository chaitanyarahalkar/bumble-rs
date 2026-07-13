use bumble::Uuid;
use bumble_att::SignedWriteSigner;
use bumble_gatt::{properties, AttServer, Characteristic, GattClient, GattServer, Service};

#[test]
fn bare_server_accepts_valid_signature_and_rejects_replay_tamper_and_wrong_key() {
    let csrk = [0x44; 16];
    let mut server = AttServer::new();
    server.set_attribute(1, b"initial".to_vec());
    server.set_signed_write_key(csrk, None);
    let mut signer = SignedWriteSigner::new(csrk, 5);

    let valid = signer.sign(1, b"accepted".to_vec()).unwrap();
    server.on_request(&valid);
    assert_eq!(server.attribute(1), Some(b"accepted".as_slice()));
    assert_eq!(server.signed_write_counter(), Some(5));

    server.set_attribute(1, b"after".to_vec());
    server.on_request(&valid);
    assert_eq!(server.attribute(1), Some(b"after".as_slice()));
    assert_eq!(server.signed_write_counter(), Some(5));

    let mut tampered = signer.sign(1, b"tampered".to_vec()).unwrap();
    if let bumble_att::AttPdu::SignedWriteCommand { signature, .. } = &mut tampered {
        signature[0] ^= 0x80;
    }
    server.on_request(&tampered);
    assert_eq!(server.attribute(1), Some(b"after".as_slice()));
    assert_eq!(server.signed_write_counter(), Some(5));

    let wrong_key = SignedWriteSigner::new([0x45; 16], 7)
        .sign(1, b"wrong".to_vec())
        .unwrap();
    server.on_request(&wrong_key);
    assert_eq!(server.attribute(1), Some(b"after".as_slice()));
}

#[test]
fn gatt_client_sends_signed_write_through_server_transport() {
    let csrk = [0x71; 16];
    let mut server = GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180F),
        characteristics: vec![Characteristic {
            uuid: Uuid::from_16_bits(0x2A19),
            properties: properties::READ | properties::AUTHENTICATED_SIGNED_WRITES,
            value: vec![1],
        }],
    }]);
    server.set_signed_write_key(csrk, None);
    let mut client = GattClient::new();
    let mut signer = SignedWriteSigner::new(csrk, 0);
    client
        .write_signed_value(&mut server, &mut signer, 3, vec![99])
        .unwrap();
    assert_eq!(client.read_value(&mut server, 3, false).unwrap(), vec![99]);
    assert_eq!(server.signed_write_counter(), Some(0));
}
