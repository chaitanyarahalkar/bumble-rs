//! HFP SDP service-record construction and discovery parsing.

use bumble::Uuid;
use bumble_sdp::{DataElement, ServiceAttribute};

use crate::{AgConfiguration, AgFeatures, AudioCodec, HfConfiguration, HfFeatures};

pub const SERVICE_RECORD_HANDLE_ATTRIBUTE_ID: u16 = 0x0000;
pub const SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID: u16 = 0x0001;
pub const PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0004;
pub const BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID: u16 = 0x0009;
pub const SUPPORTED_FEATURES_ATTRIBUTE_ID: u16 = 0x0311;

pub const L2CAP_PROTOCOL_UUID: u16 = 0x0100;
pub const RFCOMM_PROTOCOL_UUID: u16 = 0x0003;
pub const HANDSFREE_SERVICE_UUID: u16 = 0x111e;
pub const HANDSFREE_AUDIO_GATEWAY_SERVICE_UUID: u16 = 0x111f;
pub const GENERIC_AUDIO_SERVICE_UUID: u16 = 0x1203;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileVersion(pub u16);

impl ProfileVersion {
    pub const V1_5: Self = Self(0x0105);
    pub const V1_6: Self = Self(0x0106);
    pub const V1_7: Self = Self(0x0107);
    pub const V1_8: Self = Self(0x0108);
    pub const V1_9: Self = Self(0x0109);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HfSdpFeatures(pub u16);

impl HfSdpFeatures {
    pub const EC_NR: u16 = 0x01;
    pub const THREE_WAY_CALLING: u16 = 0x02;
    pub const CLI_PRESENTATION_CAPABILITY: u16 = 0x04;
    pub const VOICE_RECOGNITION_ACTIVATION: u16 = 0x08;
    pub const REMOTE_VOLUME_CONTROL: u16 = 0x10;
    pub const WIDE_BAND_SPEECH: u16 = 0x20;
    pub const ENHANCED_VOICE_RECOGNITION_STATUS: u16 = 0x40;
    pub const VOICE_RECOGNITION_TEXT: u16 = 0x80;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgSdpFeatures(pub u16);

impl AgSdpFeatures {
    pub const THREE_WAY_CALLING: u16 = 0x01;
    pub const EC_NR: u16 = 0x02;
    pub const VOICE_RECOGNITION_FUNCTION: u16 = 0x04;
    pub const IN_BAND_RING_TONE_CAPABILITY: u16 = 0x08;
    pub const VOICE_TAG: u16 = 0x10;
    pub const WIDE_BAND_SPEECH: u16 = 0x20;
    pub const ENHANCED_VOICE_RECOGNITION_STATUS: u16 = 0x40;
    pub const VOICE_RECOGNITION_TEXT: u16 = 0x80;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredHfService {
    pub rfcomm_channel: u8,
    pub version: ProfileVersion,
    pub features: HfSdpFeatures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredAgService {
    pub rfcomm_channel: u8,
    pub version: ProfileVersion,
    pub features: AgSdpFeatures,
}

pub fn make_hf_sdp_record(
    service_record_handle: u32,
    rfcomm_channel: u8,
    configuration: &HfConfiguration,
    version: ProfileVersion,
) -> Vec<ServiceAttribute> {
    let mut features = 0;
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::EC_NR,
        HfSdpFeatures::EC_NR,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::THREE_WAY_CALLING,
        HfSdpFeatures::THREE_WAY_CALLING,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::CLI_PRESENTATION_CAPABILITY,
        HfSdpFeatures::CLI_PRESENTATION_CAPABILITY,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::VOICE_RECOGNITION_ACTIVATION,
        HfSdpFeatures::VOICE_RECOGNITION_ACTIVATION,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::REMOTE_VOLUME_CONTROL,
        HfSdpFeatures::REMOTE_VOLUME_CONTROL,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::ENHANCED_VOICE_RECOGNITION_STATUS,
        HfSdpFeatures::ENHANCED_VOICE_RECOGNITION_STATUS,
    );
    map_hf(
        &mut features,
        configuration.features,
        HfFeatures::VOICE_RECOGNITION_TEXT,
        HfSdpFeatures::VOICE_RECOGNITION_TEXT,
    );
    if configuration.codecs.contains(&AudioCodec::Msbc) {
        features |= HfSdpFeatures::WIDE_BAND_SPEECH;
    }
    make_record(
        service_record_handle,
        rfcomm_channel,
        HANDSFREE_SERVICE_UUID,
        version,
        features,
    )
}

pub fn make_ag_sdp_record(
    service_record_handle: u32,
    rfcomm_channel: u8,
    configuration: &AgConfiguration,
    version: ProfileVersion,
) -> Vec<ServiceAttribute> {
    let mut features = 0;
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::EC_NR,
        AgSdpFeatures::EC_NR,
    );
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::THREE_WAY_CALLING,
        AgSdpFeatures::THREE_WAY_CALLING,
    );
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::ENHANCED_VOICE_RECOGNITION_STATUS,
        AgSdpFeatures::ENHANCED_VOICE_RECOGNITION_STATUS,
    );
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::VOICE_RECOGNITION_TEXT,
        AgSdpFeatures::VOICE_RECOGNITION_TEXT,
    );
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::IN_BAND_RING_TONE_CAPABILITY,
        AgSdpFeatures::IN_BAND_RING_TONE_CAPABILITY,
    );
    map_ag(
        &mut features,
        configuration.features,
        AgFeatures::VOICE_RECOGNITION_FUNCTION,
        AgSdpFeatures::VOICE_RECOGNITION_FUNCTION,
    );
    if configuration.codecs.contains(&AudioCodec::Msbc) {
        features |= AgSdpFeatures::WIDE_BAND_SPEECH;
    }
    make_record(
        service_record_handle,
        rfcomm_channel,
        HANDSFREE_AUDIO_GATEWAY_SERVICE_UUID,
        version,
        features,
    )
}

pub fn parse_hf_sdp_record(attributes: &[ServiceAttribute]) -> Option<DiscoveredHfService> {
    if first_service_class(attributes)? != HANDSFREE_SERVICE_UUID {
        return None;
    }
    Some(DiscoveredHfService {
        rfcomm_channel: rfcomm_channel(attributes)?,
        version: ProfileVersion(profile_version(attributes)?),
        features: HfSdpFeatures(supported_features(attributes)?),
    })
}

pub fn parse_ag_sdp_record(attributes: &[ServiceAttribute]) -> Option<DiscoveredAgService> {
    if first_service_class(attributes)? != HANDSFREE_AUDIO_GATEWAY_SERVICE_UUID {
        return None;
    }
    Some(DiscoveredAgService {
        rfcomm_channel: rfcomm_channel(attributes)?,
        version: ProfileVersion(profile_version(attributes)?),
        features: AgSdpFeatures(supported_features(attributes)?),
    })
}

fn make_record(
    service_record_handle: u32,
    rfcomm_channel: u8,
    service_uuid: u16,
    version: ProfileVersion,
    features: u16,
) -> Vec<ServiceAttribute> {
    vec![
        ServiceAttribute::new(
            SERVICE_RECORD_HANDLE_ATTRIBUTE_ID,
            DataElement::unsigned_integer_32(service_record_handle),
        ),
        ServiceAttribute::new(
            SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID,
            DataElement::sequence([
                DataElement::uuid(Uuid::from_16_bits(service_uuid)),
                DataElement::uuid(Uuid::from_16_bits(GENERIC_AUDIO_SERVICE_UUID)),
            ]),
        ),
        ServiceAttribute::new(
            PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            DataElement::sequence([
                DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(L2CAP_PROTOCOL_UUID))]),
                DataElement::sequence([
                    DataElement::uuid(Uuid::from_16_bits(RFCOMM_PROTOCOL_UUID)),
                    DataElement::unsigned_integer_8(rfcomm_channel),
                ]),
            ]),
        ),
        ServiceAttribute::new(
            BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID,
            DataElement::sequence([DataElement::sequence([
                DataElement::uuid(Uuid::from_16_bits(service_uuid)),
                DataElement::unsigned_integer_16(version.0),
            ])]),
        ),
        ServiceAttribute::new(
            SUPPORTED_FEATURES_ATTRIBUTE_ID,
            DataElement::unsigned_integer_16(features),
        ),
    ]
}

fn map_hf(target: &mut u16, source: HfFeatures, feature: HfFeatures, bit: u16) {
    if source.contains(feature) {
        *target |= bit;
    }
}

fn map_ag(target: &mut u16, source: AgFeatures, feature: AgFeatures, bit: u16) {
    if source.contains(feature) {
        *target |= bit;
    }
}

fn first_service_class(attributes: &[ServiceAttribute]) -> Option<u16> {
    let DataElement::Sequence(classes) =
        ServiceAttribute::find(attributes, SERVICE_CLASS_ID_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    uuid16(classes.first()?)
}

fn rfcomm_channel(attributes: &[ServiceAttribute]) -> Option<u8> {
    let DataElement::Sequence(protocols) =
        ServiceAttribute::find(attributes, PROTOCOL_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(rfcomm) = protocols.get(1)? else {
        return None;
    };
    if uuid16(rfcomm.first()?)? != RFCOMM_PROTOCOL_UUID {
        return None;
    }
    unsigned(rfcomm.get(1)?).and_then(|value| u8::try_from(value).ok())
}

fn profile_version(attributes: &[ServiceAttribute]) -> Option<u16> {
    let DataElement::Sequence(profiles) =
        ServiceAttribute::find(attributes, BLUETOOTH_PROFILE_DESCRIPTOR_LIST_ATTRIBUTE_ID)?
    else {
        return None;
    };
    let DataElement::Sequence(profile) = profiles.first()? else {
        return None;
    };
    unsigned(profile.get(1)?).and_then(|value| u16::try_from(value).ok())
}

fn supported_features(attributes: &[ServiceAttribute]) -> Option<u16> {
    unsigned(ServiceAttribute::find(
        attributes,
        SUPPORTED_FEATURES_ATTRIBUTE_ID,
    )?)
    .and_then(|value| u16::try_from(value).ok())
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
