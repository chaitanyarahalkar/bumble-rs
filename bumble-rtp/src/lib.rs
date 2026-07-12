//! Real-time Transport Protocol packet codec used by A2DP media streams.

use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
    TooManyCsrc,
    ExtensionNotWordAligned,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderExtension {
    pub profile: u16,
    /// Extension bytes; RTP encodes this length in 32-bit words.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaPacket {
    pub version: u8,
    pub marker: bool,
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrc_list: Vec<u32>,
    pub extension: Option<HeaderExtension>,
    pub payload: Vec<u8>,
    /// Number of trailing padding octets, including the count octet.
    pub padding_len: u8,
}

impl MediaPacket {
    pub fn new(
        payload_type: u8,
        sequence_number: u16,
        timestamp: u32,
        ssrc: u32,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: 2,
            marker: false,
            payload_type,
            sequence_number,
            timestamp,
            ssrc,
            csrc_list: Vec::new(),
            extension: None,
            payload,
            padding_len: 0,
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::Truncated("RTP fixed header"));
        }
        let version = data[0] >> 6;
        let has_padding = data[0] & 0x20 != 0;
        let has_extension = data[0] & 0x10 != 0;
        let csrc_count = usize::from(data[0] & 0x0F);
        let csrc_end = 12usize
            .checked_add(csrc_count * 4)
            .ok_or(Error::Invalid("CSRC length"))?;
        if data.len() < csrc_end {
            return Err(Error::Truncated("RTP CSRC list"));
        }
        let mut csrc_list = Vec::with_capacity(csrc_count);
        for chunk in data[12..csrc_end].chunks_exact(4) {
            csrc_list.push(u32::from_be_bytes(chunk.try_into().expect("four bytes")));
        }
        let mut payload_start = csrc_end;
        let extension = if has_extension {
            if data.len() < payload_start + 4 {
                return Err(Error::Truncated("RTP extension header"));
            }
            let profile = u16::from_be_bytes([data[payload_start], data[payload_start + 1]]);
            let words = usize::from(u16::from_be_bytes([
                data[payload_start + 2],
                data[payload_start + 3],
            ]));
            let data_start = payload_start + 4;
            let data_end = data_start
                .checked_add(words * 4)
                .ok_or(Error::Invalid("extension length"))?;
            let extension_data = data
                .get(data_start..data_end)
                .ok_or(Error::Truncated("RTP extension data"))?
                .to_vec();
            payload_start = data_end;
            Some(HeaderExtension {
                profile,
                data: extension_data,
            })
        } else {
            None
        };
        let padding_len = if has_padding {
            let padding_len = *data.last().ok_or(Error::Truncated("RTP padding"))?;
            if padding_len == 0 || usize::from(padding_len) > data.len() - payload_start {
                return Err(Error::Invalid("RTP padding length"));
            }
            padding_len
        } else {
            0
        };
        let payload_end = data.len() - usize::from(padding_len);
        Ok(Self {
            version,
            marker: data[1] & 0x80 != 0,
            payload_type: data[1] & 0x7F,
            sequence_number: u16::from_be_bytes([data[2], data[3]]),
            timestamp: u32::from_be_bytes(data[4..8].try_into().expect("four bytes")),
            ssrc: u32::from_be_bytes(data[8..12].try_into().expect("four bytes")),
            csrc_list,
            extension,
            payload: data[payload_start..payload_end].to_vec(),
            padding_len,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if self.csrc_list.len() > 15 {
            return Err(Error::TooManyCsrc);
        }
        if self.payload_type > 0x7F || self.version > 3 {
            return Err(Error::Invalid("RTP bit field"));
        }
        if self
            .extension
            .as_ref()
            .is_some_and(|extension| !extension.data.len().is_multiple_of(4))
        {
            return Err(Error::ExtensionNotWordAligned);
        }
        let mut bytes = Vec::new();
        bytes.push(
            (self.version << 6)
                | (u8::from(self.padding_len != 0) << 5)
                | (u8::from(self.extension.is_some()) << 4)
                | self.csrc_list.len() as u8,
        );
        bytes.push((u8::from(self.marker) << 7) | self.payload_type);
        bytes.extend_from_slice(&self.sequence_number.to_be_bytes());
        bytes.extend_from_slice(&self.timestamp.to_be_bytes());
        bytes.extend_from_slice(&self.ssrc.to_be_bytes());
        for csrc in &self.csrc_list {
            bytes.extend_from_slice(&csrc.to_be_bytes());
        }
        if let Some(extension) = &self.extension {
            bytes.extend_from_slice(&extension.profile.to_be_bytes());
            bytes.extend_from_slice(&((extension.data.len() / 4) as u16).to_be_bytes());
            bytes.extend_from_slice(&extension.data);
        }
        bytes.extend_from_slice(&self.payload);
        if self.padding_len != 0 {
            bytes.resize(bytes.len() + usize::from(self.padding_len) - 1, 0);
            bytes.push(self.padding_len);
        }
        Ok(bytes)
    }
}
