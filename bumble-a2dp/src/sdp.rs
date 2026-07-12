//! A2DP source/sink SDP records and discovery parsing.

use bumble::Uuid;
use bumble_sdp::{DataElement, ServiceAttribute};

use bumble_avdtp::AVDTP_PSM;

pub const SERVICE_RECORD_HANDLE_ATTRIBUTE_ID: u16 = 0x0000;
pub const SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID: u16 = 0x0001;
pub const PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0004;
pub const BROWSE_GROUP_LIST_ATTRIBUTE_ID: u16 = 0x0005;
pub const BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0009;

pub const L2CAP_PROTOCOL_UUID: u16 = 0x0100;
pub const AVDTP_PROTOCOL_UUID: u16 = 0x0019;
pub const PUBLIC_BROWSE_ROOT_UUID: u16 = 0x1002;
pub const AUDIO_SOURCE_SERVICE_UUID: u16 = 0x110A;
pub const AUDIO_SINK_SERVICE_UUID: u16 = 0x110B;
pub const ADVANCED_AUDIO_DISTRIBUTION_SERVICE_UUID: u16 = 0x110D;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileVersion(pub u16);

impl ProfileVersion {
    pub const V1_2: Self = Self(0x0102);
    pub const V1_3: Self = Self(0x0103);
    pub const V1_4: Self = Self(0x0104);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceRole {
    Source,
    Sink,
}

impl ServiceRole {
    pub fn service_uuid(self) -> u16 {
        match self {
            Self::Source => AUDIO_SOURCE_SERVICE_UUID,
            Self::Sink => AUDIO_SINK_SERVICE_UUID,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoveredService {
    pub role: ServiceRole,
    pub avdtp_version: ProfileVersion,
    pub profile_version: ProfileVersion,
}

pub fn make_audio_source_sdp_record(
    service_record_handle: u32,
    version: ProfileVersion,
) -> Vec<ServiceAttribute> {
    make_record(service_record_handle, ServiceRole::Source, version)
}

pub fn make_audio_sink_sdp_record(
    service_record_handle: u32,
    version: ProfileVersion,
) -> Vec<ServiceAttribute> {
    make_record(service_record_handle, ServiceRole::Sink, version)
}

fn make_record(
    service_record_handle: u32,
    role: ServiceRole,
    version: ProfileVersion,
) -> Vec<ServiceAttribute> {
    vec![
        ServiceAttribute::new(
            SERVICE_RECORD_HANDLE_ATTRIBUTE_ID,
            DataElement::unsigned_integer_32(service_record_handle),
        ),
        ServiceAttribute::new(
            BROWSE_GROUP_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(
                PUBLIC_BROWSE_ROOT_UUID,
            ))]),
        ),
        ServiceAttribute::new(
            SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(role.service_uuid()))]),
        ),
        ServiceAttribute::new(
            PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            DataElement::sequence([
                DataElement::sequence([
                    DataElement::uuid(Uuid::from_16_bits(L2CAP_PROTOCOL_UUID)),
                    DataElement::unsigned_integer_16(AVDTP_PSM),
                ]),
                DataElement::sequence([
                    DataElement::uuid(Uuid::from_16_bits(AVDTP_PROTOCOL_UUID)),
                    DataElement::unsigned_integer_16(version.0),
                ]),
            ]),
        ),
        ServiceAttribute::new(
            BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::sequence([
                DataElement::uuid(Uuid::from_16_bits(ADVANCED_AUDIO_DISTRIBUTION_SERVICE_UUID)),
                DataElement::unsigned_integer_16(version.0),
            ])]),
        ),
    ]
}

pub fn parse_sdp_record(attributes: &[ServiceAttribute]) -> Option<DiscoveredService> {
    let role = match first_service_class(attributes)? {
        AUDIO_SOURCE_SERVICE_UUID => ServiceRole::Source,
        AUDIO_SINK_SERVICE_UUID => ServiceRole::Sink,
        _ => return None,
    };
    let DataElement::Sequence(protocols) =
        ServiceAttribute::find(attributes, PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(l2cap) = protocols.first()? else {
        return None;
    };
    if uuid16(l2cap.first()?)? != L2CAP_PROTOCOL_UUID
        || unsigned(l2cap.get(1)?)? != u64::from(AVDTP_PSM)
    {
        return None;
    }
    let DataElement::Sequence(avdtp) = protocols.get(1)? else {
        return None;
    };
    if uuid16(avdtp.first()?)? != AVDTP_PROTOCOL_UUID {
        return None;
    }
    let avdtp_version = u16::try_from(unsigned(avdtp.get(1)?)?).ok()?;

    let DataElement::Sequence(profiles) =
        ServiceAttribute::find(attributes, BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(profile) = profiles.first()? else {
        return None;
    };
    if uuid16(profile.first()?)? != ADVANCED_AUDIO_DISTRIBUTION_SERVICE_UUID {
        return None;
    }
    let profile_version = u16::try_from(unsigned(profile.get(1)?)?).ok()?;
    Some(DiscoveredService {
        role,
        avdtp_version: ProfileVersion(avdtp_version),
        profile_version: ProfileVersion(profile_version),
    })
}

fn first_service_class(attributes: &[ServiceAttribute]) -> Option<u16> {
    let DataElement::Sequence(classes) =
        ServiceAttribute::find(attributes, SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    uuid16(classes.first()?)
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
        DataElement::UnsignedInteger { value, .. } => Some(*value),
        _ => None,
    }
}
