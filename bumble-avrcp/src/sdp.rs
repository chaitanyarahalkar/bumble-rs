//! AVRCP controller/target SDP records and discovery parsing.

use core::ops::{BitOr, BitOrAssign};

use bumble::Uuid;
use bumble_avctp::{AVCTP_BROWSING_PSM, AVCTP_PSM};
use bumble_sdp::service::{AttributeId, ClientError, SdpClient, SdpTransport};
use bumble_sdp::{DataElement, ServiceAttribute};

pub const SERVICE_RECORD_HANDLE_ATTRIBUTE_ID: u16 = 0x0000;
pub const SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID: u16 = 0x0001;
pub const PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0004;
pub const BROWSE_GROUP_LIST_ATTRIBUTE_ID: u16 = 0x0005;
pub const BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0009;
pub const ADDITIONAL_PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x000D;
pub const SUPPORTED_FEATURES_ATTRIBUTE_ID: u16 = 0x0311;

pub const L2CAP_PROTOCOL_UUID: u16 = 0x0100;
pub const AVCTP_PROTOCOL_UUID: u16 = 0x0017;
pub const PUBLIC_BROWSE_ROOT_UUID: u16 = 0x1002;
pub const AV_REMOTE_CONTROL_TARGET_SERVICE_UUID: u16 = 0x110C;
pub const AV_REMOTE_CONTROL_SERVICE_UUID: u16 = 0x110E;
pub const AV_REMOTE_CONTROL_CONTROLLER_SERVICE_UUID: u16 = 0x110F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileVersion(pub u16);

impl ProfileVersion {
    pub const V1_3: Self = Self(0x0103);
    pub const V1_4: Self = Self(0x0104);
    pub const V1_5: Self = Self(0x0105);
    pub const V1_6: Self = Self(0x0106);

    pub const fn new(major: u8, minor: u8) -> Self {
        Self(((major as u16) << 8) | minor as u16)
    }

    pub const fn major(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn minor(self) -> u8 {
        self.0 as u8
    }
}

macro_rules! feature_flags {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub struct $name(pub u16);
        impl $name {
            $(pub const $constant: Self = Self($value);)+
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }
        }
        impl BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
        }
        impl BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
        }
    };
}

feature_flags!(ControllerFeatures {
    CATEGORY_1 = 1 << 0,
    CATEGORY_2 = 1 << 1,
    CATEGORY_3 = 1 << 2,
    CATEGORY_4 = 1 << 3,
    SUPPORTS_BROWSING = 1 << 6,
    SUPPORTS_COVER_ART_GET_IMAGE_PROPERTIES_FEATURE = 1 << 7,
    SUPPORTS_COVER_ART_GET_IMAGE_FEATURE = 1 << 8,
    SUPPORTS_COVER_ART_GET_LINKED_THUMBNAIL_FEATURE = 1 << 9,
});

feature_flags!(TargetFeatures {
    CATEGORY_1 = 1 << 0,
    CATEGORY_2 = 1 << 1,
    CATEGORY_3 = 1 << 2,
    CATEGORY_4 = 1 << 3,
    PLAYER_APPLICATION_SETTINGS = 1 << 4,
    GROUP_NAVIGATION = 1 << 5,
    SUPPORTS_BROWSING = 1 << 6,
    SUPPORTS_MULTIPLE_MEDIA_PLAYER_APPLICATIONS = 1 << 7,
    SUPPORTS_COVER_ART = 1 << 8,
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerServiceSdpRecord {
    pub service_record_handle: u32,
    pub avctp_version: ProfileVersion,
    pub avrcp_version: ProfileVersion,
    pub supported_features: ControllerFeatures,
}

impl ControllerServiceSdpRecord {
    pub fn new(service_record_handle: u32) -> Self {
        Self {
            service_record_handle,
            avctp_version: ProfileVersion::V1_4,
            avrcp_version: ProfileVersion::V1_6,
            supported_features: ControllerFeatures::CATEGORY_1,
        }
    }

    pub fn to_service_attributes(&self) -> Vec<ServiceAttribute> {
        make_record(
            self.service_record_handle,
            &[
                AV_REMOTE_CONTROL_SERVICE_UUID,
                AV_REMOTE_CONTROL_CONTROLLER_SERVICE_UUID,
            ],
            self.avctp_version,
            self.avrcp_version,
            self.supported_features.0,
            self.supported_features
                .contains(ControllerFeatures::SUPPORTS_BROWSING),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetServiceSdpRecord {
    pub service_record_handle: u32,
    pub avctp_version: ProfileVersion,
    pub avrcp_version: ProfileVersion,
    pub supported_features: TargetFeatures,
}

impl TargetServiceSdpRecord {
    pub fn new(service_record_handle: u32) -> Self {
        Self {
            service_record_handle,
            avctp_version: ProfileVersion::V1_4,
            avrcp_version: ProfileVersion::V1_6,
            supported_features: TargetFeatures(0x23),
        }
    }

    pub fn to_service_attributes(&self) -> Vec<ServiceAttribute> {
        make_record(
            self.service_record_handle,
            &[AV_REMOTE_CONTROL_TARGET_SERVICE_UUID],
            self.avctp_version,
            self.avrcp_version,
            self.supported_features.0,
            self.supported_features
                .contains(TargetFeatures::SUPPORTS_BROWSING),
        )
    }
}

fn make_record(
    handle: u32,
    service_classes: &[u16],
    avctp_version: ProfileVersion,
    avrcp_version: ProfileVersion,
    supported_features: u16,
    browsing: bool,
) -> Vec<ServiceAttribute> {
    let mut attributes = vec![
        ServiceAttribute::new(
            SERVICE_RECORD_HANDLE_ATTRIBUTE_ID,
            DataElement::unsigned_integer_32(handle),
        ),
        ServiceAttribute::new(
            BROWSE_GROUP_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(
                PUBLIC_BROWSE_ROOT_UUID,
            ))]),
        ),
        ServiceAttribute::new(
            SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID,
            DataElement::sequence(
                service_classes
                    .iter()
                    .map(|uuid| DataElement::uuid(Uuid::from_16_bits(*uuid))),
            ),
        ),
        ServiceAttribute::new(
            PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            protocol_descriptors(AVCTP_PSM, avctp_version),
        ),
        ServiceAttribute::new(
            BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::sequence([
                DataElement::uuid(Uuid::from_16_bits(AV_REMOTE_CONTROL_SERVICE_UUID)),
                DataElement::unsigned_integer_16(avrcp_version.0),
            ])]),
        ),
        ServiceAttribute::new(
            SUPPORTED_FEATURES_ATTRIBUTE_ID,
            DataElement::unsigned_integer_16(supported_features),
        ),
    ];
    if browsing {
        attributes.push(ServiceAttribute::new(
            ADDITIONAL_PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            protocol_descriptors(AVCTP_BROWSING_PSM, avctp_version),
        ));
    }
    attributes
}

fn protocol_descriptors(psm: u16, version: ProfileVersion) -> DataElement {
    DataElement::sequence([
        DataElement::sequence([
            DataElement::uuid(Uuid::from_16_bits(L2CAP_PROTOCOL_UUID)),
            DataElement::unsigned_integer_16(psm),
        ]),
        DataElement::sequence([
            DataElement::uuid(Uuid::from_16_bits(AVCTP_PROTOCOL_UUID)),
            DataElement::unsigned_integer_16(version.0),
        ]),
    ])
}

pub fn parse_controller_sdp_record(
    attributes: &[ServiceAttribute],
) -> Option<ControllerServiceSdpRecord> {
    let classes = service_classes(attributes)?;
    if !classes.contains(&AV_REMOTE_CONTROL_SERVICE_UUID)
        || !classes.contains(&AV_REMOTE_CONTROL_CONTROLLER_SERVICE_UUID)
    {
        return None;
    }
    let common = parse_common(attributes)?;
    Some(ControllerServiceSdpRecord {
        service_record_handle: common.handle,
        avctp_version: common.avctp_version,
        avrcp_version: common.avrcp_version,
        supported_features: ControllerFeatures(common.supported_features),
    })
}

pub fn parse_target_sdp_record(attributes: &[ServiceAttribute]) -> Option<TargetServiceSdpRecord> {
    if !service_classes(attributes)?.contains(&AV_REMOTE_CONTROL_TARGET_SERVICE_UUID) {
        return None;
    }
    let common = parse_common(attributes)?;
    Some(TargetServiceSdpRecord {
        service_record_handle: common.handle,
        avctp_version: common.avctp_version,
        avrcp_version: common.avrcp_version,
        supported_features: TargetFeatures(common.supported_features),
    })
}

pub fn find_controller_records<T: SdpTransport>(
    client: &mut SdpClient<T>,
) -> Result<Vec<ControllerServiceSdpRecord>, ClientError> {
    Ok(client
        .service_search_attribute(
            &[Uuid::from_16_bits(
                AV_REMOTE_CONTROL_CONTROLLER_SERVICE_UUID,
            )],
            &[AttributeId::Range(0x0000, 0xFFFF)],
        )?
        .iter()
        .filter_map(|attributes| parse_controller_sdp_record(attributes))
        .collect())
}

pub fn find_target_records<T: SdpTransport>(
    client: &mut SdpClient<T>,
) -> Result<Vec<TargetServiceSdpRecord>, ClientError> {
    Ok(client
        .service_search_attribute(
            &[Uuid::from_16_bits(AV_REMOTE_CONTROL_TARGET_SERVICE_UUID)],
            &[AttributeId::Range(0x0000, 0xFFFF)],
        )?
        .iter()
        .filter_map(|attributes| parse_target_sdp_record(attributes))
        .collect())
}

struct CommonRecord {
    handle: u32,
    avctp_version: ProfileVersion,
    avrcp_version: ProfileVersion,
    supported_features: u16,
}

fn parse_common(attributes: &[ServiceAttribute]) -> Option<CommonRecord> {
    let handle = u32::try_from(unsigned(ServiceAttribute::find(
        attributes,
        SERVICE_RECORD_HANDLE_ATTRIBUTE_ID,
    )?)?)
    .ok()?;
    let DataElement::Sequence(protocols) =
        ServiceAttribute::find(attributes, PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(l2cap) = protocols.first()? else {
        return None;
    };
    if uuid16(l2cap.first()?)? != L2CAP_PROTOCOL_UUID
        || unsigned(l2cap.get(1)?)? != u64::from(AVCTP_PSM)
    {
        return None;
    }
    let DataElement::Sequence(avctp) = protocols.get(1)? else {
        return None;
    };
    if uuid16(avctp.first()?)? != AVCTP_PROTOCOL_UUID {
        return None;
    }
    let avctp_version = ProfileVersion(u16::try_from(unsigned(avctp.get(1)?)?).ok()?);

    let DataElement::Sequence(profiles) =
        ServiceAttribute::find(attributes, BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(profile) = profiles.first()? else {
        return None;
    };
    if uuid16(profile.first()?)? != AV_REMOTE_CONTROL_SERVICE_UUID {
        return None;
    }
    let avrcp_version = ProfileVersion(u16::try_from(unsigned(profile.get(1)?)?).ok()?);
    let supported_features = u16::try_from(unsigned(ServiceAttribute::find(
        attributes,
        SUPPORTED_FEATURES_ATTRIBUTE_ID,
    )?)?)
    .ok()?;
    Some(CommonRecord {
        handle,
        avctp_version,
        avrcp_version,
        supported_features,
    })
}

fn service_classes(attributes: &[ServiceAttribute]) -> Option<Vec<u16>> {
    let DataElement::Sequence(classes) =
        ServiceAttribute::find(attributes, SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    classes.iter().map(uuid16).collect()
}

fn uuid16(element: &DataElement) -> Option<u16> {
    let DataElement::Uuid(uuid) = element else {
        return None;
    };
    let bytes = uuid.to_bytes(false);
    (bytes.len() == 2).then(|| u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn unsigned(element: &DataElement) -> Option<u64> {
    match element {
        DataElement::UnsignedInteger { value, .. } => u64::try_from(*value).ok(),
        _ => None,
    }
}
