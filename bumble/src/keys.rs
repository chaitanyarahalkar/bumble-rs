//! Pairing key material and key stores, ported from `bumble.keys`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Address, AddressType};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Key {
    pub value: Vec<u8>,
    pub authenticated: bool,
    pub ediv: Option<u16>,
    pub rand: Option<Vec<u8>>,
}

impl Key {
    pub fn new(value: Vec<u8>) -> Key {
        Key {
            value,
            ..Key::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PairingKeys {
    pub address_type: Option<AddressType>,
    pub ltk: Option<Key>,
    pub ltk_central: Option<Key>,
    pub ltk_peripheral: Option<Key>,
    pub irk: Option<Key>,
    pub csrk: Option<Key>,
    pub link_key: Option<Key>,
    pub link_key_type: Option<u8>,
}

impl PairingKeys {
    pub fn to_json(&self) -> Result<String, KeyStoreError> {
        serde_json::to_string_pretty(&StoredPairingKeys::from(self)).map_err(KeyStoreError::Json)
    }

    pub fn from_json(json: &str) -> Result<Self, KeyStoreError> {
        let stored: StoredPairingKeys = serde_json::from_str(json).map_err(KeyStoreError::Json)?;
        stored.try_into()
    }
}

#[derive(Debug)]
pub enum KeyStoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidHex(String),
    InvalidAddress(String),
}

impl core::fmt::Display for KeyStoreError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for KeyStoreError {}

impl From<std::io::Error> for KeyStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub type KeyStoreResult<T> = core::result::Result<T, KeyStoreError>;

pub trait KeyStore {
    fn delete(&mut self, name: &str) -> KeyStoreResult<()>;
    fn update(&mut self, name: &str, keys: PairingKeys) -> KeyStoreResult<()>;
    fn get(&self, name: &str) -> KeyStoreResult<Option<PairingKeys>>;
    fn get_all(&self) -> KeyStoreResult<Vec<(String, PairingKeys)>>;

    fn delete_all(&mut self) -> KeyStoreResult<()> {
        for (name, _) in self.get_all()? {
            self.delete(&name)?;
        }
        Ok(())
    }

    fn get_resolving_keys(&self) -> KeyStoreResult<Vec<(Vec<u8>, Address)>> {
        self.get_all()?
            .into_iter()
            .filter_map(|(name, keys)| keys.irk.map(|irk| (name, keys.address_type, irk)))
            .map(|(name, address_type, irk)| {
                let address =
                    Address::parse(&name, address_type.unwrap_or(AddressType::RANDOM_DEVICE))
                        .map_err(|error| KeyStoreError::InvalidAddress(error.to_string()))?;
                Ok((irk.value, address))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct MemoryKeyStore {
    all_keys: BTreeMap<String, PairingKeys>,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyStore for MemoryKeyStore {
    fn delete(&mut self, name: &str) -> KeyStoreResult<()> {
        self.all_keys.remove(name);
        Ok(())
    }

    fn update(&mut self, name: &str, keys: PairingKeys) -> KeyStoreResult<()> {
        self.all_keys.insert(name.to_string(), keys);
        Ok(())
    }

    fn get(&self, name: &str) -> KeyStoreResult<Option<PairingKeys>> {
        Ok(self.all_keys.get(name).cloned())
    }

    fn get_all(&self) -> KeyStoreResult<Vec<(String, PairingKeys)>> {
        Ok(self
            .all_keys
            .iter()
            .map(|(name, keys)| (name.clone(), keys.clone()))
            .collect())
    }
}

type StoredDb = BTreeMap<String, BTreeMap<String, StoredPairingKeys>>;

#[derive(Clone, Debug)]
pub struct JsonKeyStore {
    namespace: String,
    filename: PathBuf,
}

impl JsonKeyStore {
    pub const DEFAULT_NAMESPACE: &'static str = "__DEFAULT__";

    pub fn new(namespace: Option<&str>, filename: impl Into<PathBuf>) -> Self {
        Self {
            namespace: namespace.unwrap_or(Self::DEFAULT_NAMESPACE).to_string(),
            filename: filename.into(),
        }
    }

    pub fn with_default_path(namespace: Option<&str>) -> Self {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| {
                    let home = PathBuf::from(home);
                    if cfg!(target_os = "macos") {
                        home.join("Library/Application Support")
                    } else {
                        home.join(".local/share")
                    }
                })
            })
            .unwrap_or_else(std::env::temp_dir)
            .join("Bumble/Pairing");
        let base_name = namespace.unwrap_or("keys");
        let safe_name = base_name.to_ascii_lowercase().replace([':', '/'], "-");
        Self::new(namespace, base.join(format!("{safe_name}.json")))
    }

    pub fn filename(&self) -> &Path {
        &self.filename
    }

    fn load(&self) -> KeyStoreResult<(StoredDb, String)> {
        let mut db: StoredDb = match std::fs::read(&self.filename) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(KeyStoreError::Json)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        let selected = if db.contains_key(&self.namespace) {
            self.namespace.clone()
        } else if self.namespace == Self::DEFAULT_NAMESPACE && db.len() == 1 {
            db.keys().next().expect("one namespace exists").clone()
        } else {
            db.entry(self.namespace.clone()).or_default();
            self.namespace.clone()
        };
        Ok((db, selected))
    }

    fn save(&self, db: &StoredDb) -> KeyStoreResult<()> {
        if let Some(parent) = self.filename.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut temp = self.filename.as_os_str().to_os_string();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        let bytes = serde_json::to_vec_pretty(db).map_err(KeyStoreError::Json)?;
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, &self.filename)?;
        Ok(())
    }
}

impl KeyStore for JsonKeyStore {
    fn delete(&mut self, name: &str) -> KeyStoreResult<()> {
        let (mut db, selected) = self.load()?;
        db.entry(selected).or_default().remove(name);
        self.save(&db)
    }

    fn update(&mut self, name: &str, keys: PairingKeys) -> KeyStoreResult<()> {
        let (mut db, selected) = self.load()?;
        let stored = StoredPairingKeys::from(&keys);
        db.entry(selected)
            .or_default()
            .entry(name.to_string())
            .and_modify(|existing| existing.merge(stored.clone()))
            .or_insert(stored);
        self.save(&db)
    }

    fn get(&self, name: &str) -> KeyStoreResult<Option<PairingKeys>> {
        let (db, selected) = self.load()?;
        db.get(&selected)
            .and_then(|keys| keys.get(name))
            .cloned()
            .map(TryInto::try_into)
            .transpose()
    }

    fn get_all(&self) -> KeyStoreResult<Vec<(String, PairingKeys)>> {
        let (db, selected) = self.load()?;
        db.get(&selected)
            .into_iter()
            .flat_map(|keys| keys.iter())
            .map(|(name, keys)| Ok((name.clone(), keys.clone().try_into()?)))
            .collect()
    }

    fn delete_all(&mut self) -> KeyStoreResult<()> {
        let (mut db, selected) = self.load()?;
        db.entry(selected).or_default().clear();
        self.save(&db)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredKey {
    value: String,
    #[serde(default)]
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ediv: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rand: Option<String>,
}

impl From<&Key> for StoredKey {
    fn from(key: &Key) -> Self {
        Self {
            value: hex(&key.value),
            authenticated: key.authenticated,
            ediv: key.ediv,
            rand: key.rand.as_deref().map(hex),
        }
    }
}

impl TryFrom<StoredKey> for Key {
    type Error = KeyStoreError;

    fn try_from(key: StoredKey) -> KeyStoreResult<Self> {
        Ok(Self {
            value: decode_hex(&key.value)?,
            authenticated: key.authenticated,
            ediv: key.ediv,
            rand: key.rand.map(|value| decode_hex(&value)).transpose()?,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredPairingKeys {
    #[serde(skip_serializing_if = "Option::is_none")]
    address_type: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ltk: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ltk_central: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ltk_peripheral: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    irk: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    csrk: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_key: Option<StoredKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_key_type: Option<u8>,
}

impl StoredPairingKeys {
    fn merge(&mut self, newer: Self) {
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {
                $(if newer.$field.is_some() { self.$field = newer.$field; })+
            };
        }
        merge!(
            address_type,
            ltk,
            ltk_central,
            ltk_peripheral,
            irk,
            csrk,
            link_key,
            link_key_type,
        );
    }
}

impl From<&PairingKeys> for StoredPairingKeys {
    fn from(keys: &PairingKeys) -> Self {
        Self {
            address_type: keys.address_type.map(|value| value.0),
            ltk: keys.ltk.as_ref().map(StoredKey::from),
            ltk_central: keys.ltk_central.as_ref().map(StoredKey::from),
            ltk_peripheral: keys.ltk_peripheral.as_ref().map(StoredKey::from),
            irk: keys.irk.as_ref().map(StoredKey::from),
            csrk: keys.csrk.as_ref().map(StoredKey::from),
            link_key: keys.link_key.as_ref().map(StoredKey::from),
            link_key_type: keys.link_key_type,
        }
    }
}

impl TryFrom<StoredPairingKeys> for PairingKeys {
    type Error = KeyStoreError;

    fn try_from(keys: StoredPairingKeys) -> KeyStoreResult<Self> {
        Ok(Self {
            address_type: keys.address_type.map(AddressType),
            ltk: keys.ltk.map(TryInto::try_into).transpose()?,
            ltk_central: keys.ltk_central.map(TryInto::try_into).transpose()?,
            ltk_peripheral: keys.ltk_peripheral.map(TryInto::try_into).transpose()?,
            irk: keys.irk.map(TryInto::try_into).transpose()?,
            csrk: keys.csrk.map(TryInto::try_into).transpose()?,
            link_key: keys.link_key.map(TryInto::try_into).transpose()?,
            link_key_type: keys.link_key_type,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> KeyStoreResult<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(KeyStoreError::InvalidHex(value.to_string()));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| KeyStoreError::InvalidHex(value.to_string()))?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| KeyStoreError::InvalidHex(value.to_string()))?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_json_matches_upstream_shape() {
        let keys = PairingKeys {
            address_type: Some(AddressType::RANDOM_DEVICE),
            ltk: Some(Key {
                value: vec![0xAA; 16],
                authenticated: true,
                ediv: Some(0x1234),
                rand: Some(vec![0xBB; 8]),
            }),
            irk: Some(Key::new(vec![0xCC; 16])),
            ..PairingKeys::default()
        };
        let json = keys.to_json().unwrap();
        assert!(json.contains("\"address_type\": 1"));
        assert!(json.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert_eq!(PairingKeys::from_json(&json).unwrap(), keys);
    }

    #[test]
    fn memory_store_and_resolving_keys() {
        let mut store = MemoryKeyStore::new();
        let keys = PairingKeys {
            address_type: Some(AddressType::RANDOM_IDENTITY),
            irk: Some(Key::new(vec![7; 16])),
            ..PairingKeys::default()
        };
        store.update("C4:F2:17:1A:1D:BB", keys.clone()).unwrap();
        assert_eq!(store.get("C4:F2:17:1A:1D:BB").unwrap(), Some(keys));
        let resolving = store.get_resolving_keys().unwrap();
        assert_eq!(resolving.len(), 1);
        assert_eq!(resolving[0].0, vec![7; 16]);
        assert_eq!(resolving[0].1.address_type(), AddressType::RANDOM_IDENTITY);
        store.delete_all().unwrap();
        assert!(store.get_all().unwrap().is_empty());
    }
}
