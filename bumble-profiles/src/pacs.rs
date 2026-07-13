//! Published Audio Capabilities Service (PACS).

use crate::bap::{AudioLocation, CodecSpecificCapabilities, ContextType};
use crate::le_audio::Metadata;
use crate::{discover_profile, find_characteristic, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    GattClient, ServiceDefinition, ServiceProxy,
};
use bumble_hci::CodingFormat;

pub const PUBLISHED_AUDIO_CAPABILITIES_SERVICE: u16 = 0x1850;
pub const SINK_PAC_CHARACTERISTIC: u16 = 0x2BC9;
pub const SINK_AUDIO_LOCATION_CHARACTERISTIC: u16 = 0x2BCA;
pub const SOURCE_PAC_CHARACTERISTIC: u16 = 0x2BCB;
pub const SOURCE_AUDIO_LOCATION_CHARACTERISTIC: u16 = 0x2BCC;
pub const AVAILABLE_AUDIO_CONTEXTS_CHARACTERISTIC: u16 = 0x2BCD;
pub const SUPPORTED_AUDIO_CONTEXTS_CHARACTERISTIC: u16 = 0x2BCE;
pub const VENDOR_SPECIFIC_CODEC_ID: u8 = 0xFF;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PacCodecCapabilities {
    Standard(CodecSpecificCapabilities),
    VendorSpecific(Vec<u8>),
}

impl PacCodecCapabilities {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::Standard(capabilities) => capabilities.to_bytes(),
            Self::VendorSpecific(value) => Ok(value.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PacRecord {
    pub coding_format: CodingFormat,
    pub codec_specific_capabilities: PacCodecCapabilities,
    pub metadata: Metadata,
}

impl PacRecord {
    pub fn from_bytes(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 7 {
            return Err(Error::InvalidValue("PAC record is truncated".into()));
        }
        let coding_format = CodingFormat::from_bytes(&data[..5])
            .map_err(|error| Error::InvalidValue(format!("invalid coding format: {error}")))?;
        let capabilities_size = usize::from(data[5]);
        let capabilities_end = 6usize
            .checked_add(capabilities_size)
            .ok_or_else(|| Error::InvalidValue("PAC capability length overflow".into()))?;
        if capabilities_end >= data.len() {
            return Err(Error::InvalidValue(
                "PAC record omits capability or metadata bytes".into(),
            ));
        }
        let metadata_size = usize::from(data[capabilities_end]);
        let metadata_end = capabilities_end
            .checked_add(1 + metadata_size)
            .ok_or_else(|| Error::InvalidValue("PAC metadata length overflow".into()))?;
        if metadata_end > data.len() {
            return Err(Error::InvalidValue("PAC metadata is truncated".into()));
        }
        let capability_bytes = &data[6..capabilities_end];
        let codec_specific_capabilities = if coding_format.coding_format == VENDOR_SPECIFIC_CODEC_ID
        {
            PacCodecCapabilities::VendorSpecific(capability_bytes.to_vec())
        } else {
            PacCodecCapabilities::Standard(CodecSpecificCapabilities::from_bytes(capability_bytes)?)
        };
        Ok((
            Self {
                coding_format,
                codec_specific_capabilities,
                metadata: Metadata::from_bytes(&data[capabilities_end + 1..metadata_end])?,
            },
            metadata_end,
        ))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let capabilities = self.codec_specific_capabilities.to_bytes()?;
        let metadata = self.metadata.to_bytes()?;
        let capabilities_length = u8::try_from(capabilities.len())
            .map_err(|_| Error::InvalidValue("PAC capabilities exceed 255 bytes".into()))?;
        let metadata_length = u8::try_from(metadata.len())
            .map_err(|_| Error::InvalidValue("PAC metadata exceeds 255 bytes".into()))?;
        let mut value = self.coding_format.to_bytes().to_vec();
        value.push(capabilities_length);
        value.extend_from_slice(&capabilities);
        value.push(metadata_length);
        value.extend_from_slice(&metadata);
        Ok(value)
    }

    pub fn list_from_bytes(data: &[u8]) -> Result<Vec<Self>> {
        let (&record_count, data) = data
            .split_first()
            .ok_or_else(|| Error::InvalidValue("PAC list is empty".into()))?;
        let mut offset = 0;
        let mut records = Vec::with_capacity(usize::from(record_count));
        for _ in 0..record_count {
            let (record, consumed) = Self::from_bytes(&data[offset..])?;
            offset = offset
                .checked_add(consumed)
                .ok_or_else(|| Error::InvalidValue("PAC list length overflow".into()))?;
            records.push(record);
        }
        if offset != data.len() {
            return Err(Error::InvalidValue(format!(
                "PAC list has {} trailing bytes",
                data.len() - offset
            )));
        }
        Ok(records)
    }

    pub fn list_to_bytes(records: &[Self]) -> Result<Vec<u8>> {
        let count = u8::try_from(records.len())
            .map_err(|_| Error::InvalidValue("PAC list exceeds 255 records".into()))?;
        let mut value = vec![count];
        for record in records {
            value.extend_from_slice(&record.to_bytes()?);
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioContexts {
    pub sink: ContextType,
    pub source: ContextType,
}

impl AudioContexts {
    pub fn encode(self) -> [u8; 4] {
        let sink = self.sink.0.to_le_bytes();
        let source = self.source.0.to_le_bytes();
        [sink[0], sink[1], source[0], source[1]]
    }

    pub fn decode(value: &[u8]) -> Result<Self> {
        if value.len() != 4 {
            return Err(Error::InvalidValue(format!(
                "audio contexts have length {}, expected 4",
                value.len()
            )));
        }
        Ok(Self {
            sink: ContextType(u16::from_le_bytes([value[0], value[1]])),
            source: ContextType(u16::from_le_bytes([value[2], value[3]])),
        })
    }
}

#[derive(Clone, Debug)]
pub struct PublishedAudioCapabilitiesService {
    pub supported_contexts: AudioContexts,
    pub available_contexts: AudioContexts,
    pub sink_pac: Vec<PacRecord>,
    pub sink_audio_locations: Option<AudioLocation>,
    pub source_pac: Vec<PacRecord>,
    pub source_audio_locations: Option<AudioLocation>,
}

impl PublishedAudioCapabilitiesService {
    pub fn new(supported_contexts: AudioContexts, available_contexts: AudioContexts) -> Self {
        Self {
            supported_contexts,
            available_contexts,
            sink_pac: Vec::new(),
            sink_audio_locations: None,
            source_pac: Vec::new(),
            source_audio_locations: None,
        }
    }

    pub fn definition(&self) -> Result<ServiceDefinition> {
        let mut characteristics = vec![
            characteristic(
                SUPPORTED_AUDIO_CONTEXTS_CHARACTERISTIC,
                properties::READ,
                self.supported_contexts.encode().to_vec(),
            ),
            characteristic(
                AVAILABLE_AUDIO_CONTEXTS_CHARACTERISTIC,
                properties::READ | properties::NOTIFY,
                self.available_contexts.encode().to_vec(),
            ),
        ];
        if !self.sink_pac.is_empty() {
            characteristics.push(characteristic(
                SINK_PAC_CHARACTERISTIC,
                properties::READ,
                PacRecord::list_to_bytes(&self.sink_pac)?,
            ));
        }
        if let Some(location) = self.sink_audio_locations {
            characteristics.push(characteristic(
                SINK_AUDIO_LOCATION_CHARACTERISTIC,
                properties::READ,
                location.0.to_le_bytes().to_vec(),
            ));
        }
        if !self.source_pac.is_empty() {
            characteristics.push(characteristic(
                SOURCE_PAC_CHARACTERISTIC,
                properties::READ,
                PacRecord::list_to_bytes(&self.source_pac)?,
            ));
        }
        if let Some(location) = self.source_audio_locations {
            characteristics.push(characteristic(
                SOURCE_AUDIO_LOCATION_CHARACTERISTIC,
                properties::READ,
                location.0.to_le_bytes().to_vec(),
            ));
        }
        Ok(ServiceDefinition {
            uuid: uuid(PUBLISHED_AUDIO_CAPABILITIES_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        })
    }
}

fn characteristic(
    characteristic_uuid: u16,
    characteristic_properties: u8,
    value: Vec<u8>,
) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: characteristic_properties,
        permissions: permissions::READABLE,
        value,
        descriptors: Vec::new(),
    }
}

#[derive(Clone, Debug)]
pub struct PublishedAudioCapabilitiesServiceProxy {
    pub service: ServiceProxy,
    pub sink_pac: Option<CharacteristicProxy>,
    pub sink_audio_locations: Option<CharacteristicProxy>,
    pub source_pac: Option<CharacteristicProxy>,
    pub source_audio_locations: Option<CharacteristicProxy>,
    pub available_audio_contexts: CharacteristicProxy,
    pub supported_audio_contexts: CharacteristicProxy,
}

impl PublishedAudioCapabilitiesServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            sink_pac: find_characteristic(characteristics, SINK_PAC_CHARACTERISTIC),
            sink_audio_locations: find_characteristic(
                characteristics,
                SINK_AUDIO_LOCATION_CHARACTERISTIC,
            ),
            source_pac: find_characteristic(characteristics, SOURCE_PAC_CHARACTERISTIC),
            source_audio_locations: find_characteristic(
                characteristics,
                SOURCE_AUDIO_LOCATION_CHARACTERISTIC,
            ),
            available_audio_contexts: require_characteristic(
                characteristics,
                AVAILABLE_AUDIO_CONTEXTS_CHARACTERISTIC,
            )?,
            supported_audio_contexts: require_characteristic(
                characteristics,
                SUPPORTED_AUDIO_CONTEXTS_CHARACTERISTIC,
            )?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, PUBLISHED_AUDIO_CAPABILITIES_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_available_contexts(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioContexts> {
        AudioContexts::decode(&client.read_value(
            transport,
            self.available_audio_contexts.handle,
            false,
        )?)
    }

    pub fn read_supported_contexts(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioContexts> {
        AudioContexts::decode(&client.read_value(
            transport,
            self.supported_audio_contexts.handle,
            false,
        )?)
    }

    pub fn read_pac(
        characteristic: &CharacteristicProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Vec<PacRecord>> {
        PacRecord::list_from_bytes(&client.read_value(transport, characteristic.handle, false)?)
    }

    pub fn read_audio_locations(
        characteristic: &CharacteristicProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<AudioLocation> {
        let value = client.read_value(transport, characteristic.handle, false)?;
        let bytes: [u8; 4] = value.try_into().map_err(|value: Vec<u8>| {
            Error::InvalidValue(format!(
                "audio locations have length {}, expected 4",
                value.len()
            ))
        })?;
        Ok(AudioLocation(u32::from_le_bytes(bytes)))
    }
}
