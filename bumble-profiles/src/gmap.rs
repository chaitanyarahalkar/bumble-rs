//! Gaming Audio Profile (GMAP) role and feature service.

use crate::{discover_profile, find_characteristic, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    GattClient, ServiceDefinition, ServiceProxy,
};
use std::ops::{BitOr, BitOrAssign};

pub const GAMING_AUDIO_SERVICE: u16 = 0x1858;
pub const GMAP_ROLE_CHARACTERISTIC: u16 = 0x2C00;
pub const UGG_FEATURES_CHARACTERISTIC: u16 = 0x2C01;
pub const UGT_FEATURES_CHARACTERISTIC: u16 = 0x2C02;
pub const BGS_FEATURES_CHARACTERISTIC: u16 = 0x2C03;
pub const BGR_FEATURES_CHARACTERISTIC: u16 = 0x2C04;

macro_rules! flags {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+

            pub fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }
        }

        impl BitOr for $name {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self(self.0 | rhs.0)
            }
        }

        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }
    };
}

flags!(GmapRole {
    UNICAST_GAME_GATEWAY = 1 << 0,
    UNICAST_GAME_TERMINAL = 1 << 1,
    BROADCAST_GAME_SENDER = 1 << 2,
    BROADCAST_GAME_RECEIVER = 1 << 3,
});

flags!(UggFeatures {
    UGG_MULTIPLEX = 1 << 0,
    UGG_96_KBPS_SOURCE = 1 << 1,
    UGG_MULTISINK = 1 << 2,
});

flags!(UgtFeatures {
    UGT_SOURCE = 1 << 0,
    UGT_80_KBPS_SOURCE = 1 << 1,
    UGT_SINK = 1 << 2,
    UGT_64_KBPS_SINK = 1 << 3,
    UGT_MULTIPLEX = 1 << 4,
    UGT_MULTISINK = 1 << 5,
    UGT_MULTISOURCE = 1 << 6,
});

flags!(BgsFeatures {
    BGS_96_KBPS = 1 << 0,
});

flags!(BgrFeatures {
    BGR_MULTISINK = 1 << 0,
    BGR_MULTIPLEX = 1 << 1,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamingAudioService {
    pub gmap_role: GmapRole,
    pub ugg_features: UggFeatures,
    pub ugt_features: UgtFeatures,
    pub bgs_features: BgsFeatures,
    pub bgr_features: BgrFeatures,
}

impl GamingAudioService {
    pub fn new(gmap_role: GmapRole) -> Self {
        Self {
            gmap_role,
            ugg_features: UggFeatures::default(),
            ugt_features: UgtFeatures::default(),
            bgs_features: BgsFeatures::default(),
            bgr_features: BgrFeatures::default(),
        }
    }

    pub fn definition(self) -> ServiceDefinition {
        let mut characteristics = vec![characteristic(GMAP_ROLE_CHARACTERISTIC, self.gmap_role.0)];
        if self.gmap_role.contains(GmapRole::UNICAST_GAME_GATEWAY) {
            characteristics.push(characteristic(
                UGG_FEATURES_CHARACTERISTIC,
                self.ugg_features.0,
            ));
        }
        if self.gmap_role.contains(GmapRole::UNICAST_GAME_TERMINAL) {
            characteristics.push(characteristic(
                UGT_FEATURES_CHARACTERISTIC,
                self.ugt_features.0,
            ));
        }
        if self.gmap_role.contains(GmapRole::BROADCAST_GAME_SENDER) {
            characteristics.push(characteristic(
                BGS_FEATURES_CHARACTERISTIC,
                self.bgs_features.0,
            ));
        }
        if self.gmap_role.contains(GmapRole::BROADCAST_GAME_RECEIVER) {
            characteristics.push(characteristic(
                BGR_FEATURES_CHARACTERISTIC,
                self.bgr_features.0,
            ));
        }
        ServiceDefinition {
            uuid: uuid(GAMING_AUDIO_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        }
    }
}

fn characteristic(characteristic_uuid: u16, value: u8) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: properties::READ,
        permissions: permissions::READABLE,
        value: vec![value],
        descriptors: Vec::new(),
    }
}

#[derive(Clone, Debug)]
pub struct GamingAudioServiceProxy {
    pub service: ServiceProxy,
    pub gmap_role: CharacteristicProxy,
    pub ugg_features: Option<CharacteristicProxy>,
    pub ugt_features: Option<CharacteristicProxy>,
    pub bgs_features: Option<CharacteristicProxy>,
    pub bgr_features: Option<CharacteristicProxy>,
}

impl GamingAudioServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            gmap_role: require_characteristic(characteristics, GMAP_ROLE_CHARACTERISTIC)?,
            ugg_features: find_characteristic(characteristics, UGG_FEATURES_CHARACTERISTIC),
            ugt_features: find_characteristic(characteristics, UGT_FEATURES_CHARACTERISTIC),
            bgs_features: find_characteristic(characteristics, BGS_FEATURES_CHARACTERISTIC),
            bgr_features: find_characteristic(characteristics, BGR_FEATURES_CHARACTERISTIC),
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, GAMING_AUDIO_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_role(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<GmapRole> {
        read_flag(&self.gmap_role, client, transport).map(GmapRole)
    }

    pub fn read_ugg_features(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<UggFeatures>> {
        read_optional_flag(self.ugg_features.as_ref(), client, transport)
            .map(|value| value.map(UggFeatures))
    }

    pub fn read_ugt_features(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<UgtFeatures>> {
        read_optional_flag(self.ugt_features.as_ref(), client, transport)
            .map(|value| value.map(UgtFeatures))
    }

    pub fn read_bgs_features(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<BgsFeatures>> {
        read_optional_flag(self.bgs_features.as_ref(), client, transport)
            .map(|value| value.map(BgsFeatures))
    }

    pub fn read_bgr_features(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<BgrFeatures>> {
        read_optional_flag(self.bgr_features.as_ref(), client, transport)
            .map(|value| value.map(BgrFeatures))
    }
}

fn read_optional_flag(
    characteristic: Option<&CharacteristicProxy>,
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<Option<u8>> {
    characteristic
        .map(|characteristic| read_flag(characteristic, client, transport))
        .transpose()
}

fn read_flag(
    characteristic: &CharacteristicProxy,
    client: &mut GattClient,
    transport: &mut impl AttTransport,
) -> Result<u8> {
    let value = client.read_value(transport, characteristic.handle, false)?;
    let [value]: [u8; 1] = value.try_into().map_err(|value: Vec<u8>| {
        Error::InvalidValue(format!(
            "GMAP role/feature has length {}, expected 1",
            value.len()
        ))
    })?;
    Ok(value)
}
