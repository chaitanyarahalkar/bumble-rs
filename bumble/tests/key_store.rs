use std::time::{SystemTime, UNIX_EPOCH};

use bumble::keys::{JsonKeyStore, Key, KeyStore, PairingKeys};
use bumble::AddressType;

fn path(test_name: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bumble-rs-keys-{}-{test_name}-{unique}.json",
        std::process::id(),
    ))
}

#[test]
fn json_store_is_atomic_namespaced_and_merges_partial_updates() {
    let filename = path("atomic");
    let mut first = JsonKeyStore::new(Some("controller-a"), &filename);
    first
        .update(
            "C4:F2:17:1A:1D:BB",
            PairingKeys {
                address_type: Some(AddressType::RANDOM_DEVICE),
                ltk: Some(Key::new(vec![1; 16])),
                irk: Some(Key::new(vec![2; 16])),
                ..PairingKeys::default()
            },
        )
        .unwrap();
    first
        .update(
            "C4:F2:17:1A:1D:BB",
            PairingKeys {
                csrk: Some(Key::new(vec![3; 16])),
                ..PairingKeys::default()
            },
        )
        .unwrap();
    let merged = first.get("C4:F2:17:1A:1D:BB").unwrap().unwrap();
    assert_eq!(merged.ltk.unwrap().value, vec![1; 16]);
    assert_eq!(merged.irk.unwrap().value, vec![2; 16]);
    assert_eq!(merged.csrk.unwrap().value, vec![3; 16]);

    let fallback = JsonKeyStore::new(None, &filename);
    assert!(fallback.get("C4:F2:17:1A:1D:BB").unwrap().is_some());
    let mut fallback = fallback;
    assert!(matches!(
        fallback.delete("missing"),
        Err(bumble::keys::KeyStoreError::NotFound(name)) if name == "missing"
    ));
    assert!(!filename.with_extension("json.tmp").exists());
    std::fs::remove_file(filename).unwrap();
}

#[test]
fn corrupt_json_and_invalid_hex_are_reported() {
    let filename = path("invalid");
    std::fs::write(&filename, b"not-json").unwrap();
    let store = JsonKeyStore::new(None, &filename);
    assert!(store.get_all().is_err());
    std::fs::write(
        &filename,
        br#"{"__DEFAULT__":{"peer":{"ltk":{"value":"xyz","authenticated":false}}}}"#,
    )
    .unwrap();
    assert!(store.get("peer").is_err());
    std::fs::remove_file(filename).unwrap();
}
