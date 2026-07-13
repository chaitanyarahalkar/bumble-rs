//! Public Broadcast Profile announcement data.

use crate::le_audio::Metadata;
use crate::{Error, Result};
use bumble::{advertising_data::Type as AdvertisingType, AdvertisingData, Uuid};
use std::ops::{BitOr, BitOrAssign};

pub const PUBLIC_BROADCAST_ANNOUNCEMENT_SERVICE: u16 = 0x1856;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PublicBroadcastFeatures(pub u8);

impl PublicBroadcastFeatures {
    pub const ENCRYPTED: Self = Self(1 << 0);
    pub const STANDARD_QUALITY_CONFIGURATION: Self = Self(1 << 1);
    pub const HIGH_QUALITY_CONFIGURATION: Self = Self(1 << 2);
}

impl BitOr for PublicBroadcastFeatures {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PublicBroadcastFeatures {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicBroadcastAnnouncement {
    pub features: PublicBroadcastFeatures,
    pub metadata: Metadata,
}

impl PublicBroadcastAnnouncement {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::InvalidValue(
                "public broadcast announcement is truncated".into(),
            ));
        }
        let metadata_length = usize::from(data[1]);
        if data.len() != metadata_length + 2 {
            return Err(Error::InvalidValue(format!(
                "public broadcast metadata length {} does not match {} bytes",
                metadata_length,
                data.len().saturating_sub(2)
            )));
        }
        Ok(Self {
            features: PublicBroadcastFeatures(data[0]),
            metadata: Metadata::from_bytes(&data[2..])?,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let metadata = self.metadata.to_bytes()?;
        let length = u8::try_from(metadata.len()).map_err(|_| {
            Error::InvalidValue("public broadcast metadata exceeds 255 bytes".into())
        })?;
        let mut value = vec![self.features.0, length];
        value.extend_from_slice(&metadata);
        Ok(value)
    }

    pub fn advertising_data(&self) -> Result<Vec<u8>> {
        let mut value = Uuid::from_16_bits(PUBLIC_BROADCAST_ANNOUNCEMENT_SERVICE).to_bytes(false);
        value.extend_from_slice(&self.to_bytes()?);
        Ok(AdvertisingData {
            ad_structures: vec![(AdvertisingType(0x16), value)],
        }
        .to_bytes())
    }
}
