//! Audio/Video Remote Control Profile (AVRCP) codecs and protocol helpers.

use core::fmt;

mod command;
mod event;
mod response;

pub use command::*;
pub use event::*;
pub use response::*;

pub const AVRCP_PID: u16 = 0x110E;
pub const BLUETOOTH_SIG_COMPANY_ID: u32 = 0x001958;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PduId(pub u8);

impl PduId {
    pub const GET_CAPABILITIES: Self = Self(0x10);
    pub const LIST_PLAYER_APPLICATION_SETTING_ATTRIBUTES: Self = Self(0x11);
    pub const LIST_PLAYER_APPLICATION_SETTING_VALUES: Self = Self(0x12);
    pub const GET_CURRENT_PLAYER_APPLICATION_SETTING_VALUE: Self = Self(0x13);
    pub const SET_PLAYER_APPLICATION_SETTING_VALUE: Self = Self(0x14);
    pub const GET_PLAYER_APPLICATION_SETTING_ATTRIBUTE_TEXT: Self = Self(0x15);
    pub const GET_PLAYER_APPLICATION_SETTING_VALUE_TEXT: Self = Self(0x16);
    pub const INFORM_DISPLAYABLE_CHARACTER_SET: Self = Self(0x17);
    pub const INFORM_BATTERY_STATUS_OF_CT: Self = Self(0x18);
    pub const GET_ELEMENT_ATTRIBUTES: Self = Self(0x20);
    pub const GET_PLAY_STATUS: Self = Self(0x30);
    pub const REGISTER_NOTIFICATION: Self = Self(0x31);
    pub const REQUEST_CONTINUING_RESPONSE: Self = Self(0x40);
    pub const ABORT_CONTINUING_RESPONSE: Self = Self(0x41);
    pub const SET_ABSOLUTE_VOLUME: Self = Self(0x50);
    pub const SET_ADDRESSED_PLAYER: Self = Self(0x60);
    pub const SET_BROWSED_PLAYER: Self = Self(0x70);
    pub const GET_FOLDER_ITEMS: Self = Self(0x71);
    pub const CHANGE_PATH: Self = Self(0x72);
    pub const GET_ITEM_ATTRIBUTES: Self = Self(0x73);
    pub const PLAY_ITEM: Self = Self(0x74);
    pub const GET_TOTAL_NUMBER_OF_ITEMS: Self = Self(0x75);
    pub const SEARCH: Self = Self(0x80);
    pub const ADD_TO_NOW_PLAYING: Self = Self(0x90);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    Single = 0,
    Start = 1,
    Continue = 2,
    End = 3,
}

impl PacketType {
    fn from_byte(value: u8) -> Self {
        match value & 3 {
            0 => Self::Single,
            1 => Self::Start,
            2 => Self::Continue,
            3 => Self::End,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    LengthMismatch { declared: usize, actual: usize },
    ParametersTooLong,
    InvalidFragmentSize,
    WrongCompanyId(u32),
    WrongPid(u16),
    NotVendorDependent,
    InvalidField(&'static str),
    TrailingBytes(usize),
    Avc(bumble_avc::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl From<bumble_avc::Error> for Error {
    fn from(error: bumble_avc::Error) -> Self {
        Self::Avc(error)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// One AVRCP vendor-dependent PDU fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorPdu {
    pub pdu_id: PduId,
    pub packet_type: PacketType,
    pub parameters: Vec<u8>,
}

impl VendorPdu {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            return Err(Error::Truncated("AVRCP PDU header"));
        }
        let declared = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        let actual = bytes.len() - 4;
        if declared != actual {
            return Err(Error::LengthMismatch { declared, actual });
        }
        Ok(Self {
            pdu_id: PduId(bytes[0]),
            packet_type: PacketType::from_byte(bytes[1]),
            parameters: bytes[4..].to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let length = u16::try_from(self.parameters.len()).map_err(|_| Error::ParametersTooLong)?;
        let mut bytes = Vec::with_capacity(4 + self.parameters.len());
        bytes.push(self.pdu_id.0);
        bytes.push(self.packet_type as u8);
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&self.parameters);
        Ok(bytes)
    }
}

/// Stateful reassembler for AVRCP's independent vendor-PDU fragmentation.
#[derive(Clone, Debug, Default)]
pub struct PduAssembler {
    pending: Option<(PduId, Vec<u8>)>,
}

impl PduAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.pending = None;
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Option<(PduId, Vec<u8>)>> {
        let pdu = match VendorPdu::from_bytes(bytes) {
            Ok(pdu) => pdu,
            Err(error) => {
                self.reset();
                return Err(error);
            }
        };

        match pdu.packet_type {
            PacketType::Single => {
                self.reset();
                Ok(Some((pdu.pdu_id, pdu.parameters)))
            }
            PacketType::Start => {
                self.pending = Some((pdu.pdu_id, pdu.parameters));
                Ok(None)
            }
            PacketType::Continue => {
                let Some((pending_id, parameters)) = &mut self.pending else {
                    return Ok(None);
                };
                if *pending_id != pdu.pdu_id {
                    self.reset();
                    return Ok(None);
                }
                parameters.extend_from_slice(&pdu.parameters);
                Ok(None)
            }
            PacketType::End => {
                let Some((pending_id, _)) = &self.pending else {
                    return Ok(None);
                };
                if *pending_id != pdu.pdu_id {
                    self.reset();
                    return Ok(None);
                }
                let (pdu_id, mut parameters) = self.pending.take().expect("pending PDU checked");
                parameters.extend_from_slice(&pdu.parameters);
                Ok(Some((pdu_id, parameters)))
            }
        }
    }
}

/// Splits a complete AVRCP PDU into fragments whose parameter sections fit
/// `max_parameters` bytes.
pub fn fragment_pdu(
    pdu_id: PduId,
    parameters: &[u8],
    max_parameters: usize,
) -> Result<Vec<VendorPdu>> {
    if max_parameters == 0 {
        return Err(Error::InvalidFragmentSize);
    }
    if parameters.len() <= max_parameters {
        return Ok(vec![VendorPdu {
            pdu_id,
            packet_type: PacketType::Single,
            parameters: parameters.to_vec(),
        }]);
    }

    let chunk_count = parameters.len().div_ceil(max_parameters);
    Ok(parameters
        .chunks(max_parameters)
        .enumerate()
        .map(|(index, chunk)| VendorPdu {
            pdu_id,
            packet_type: if index == 0 {
                PacketType::Start
            } else if index + 1 == chunk_count {
                PacketType::End
            } else {
                PacketType::Continue
            },
            parameters: chunk.to_vec(),
        })
        .collect())
}

/// Wraps one AVRCP vendor PDU in the AV/C panel command envelope used by AVRCP.
pub fn command_frame(
    command_type: bumble_avc::CommandType,
    pdu: &VendorPdu,
) -> Result<bumble_avc::Frame> {
    Ok(bumble_avc::Frame::Command {
        command_type,
        subunit_type: bumble_avc::SubunitType::PANEL,
        subunit_id: 0,
        body: bumble_avc::FrameBody::VendorDependent {
            company_id: BLUETOOTH_SIG_COMPANY_ID,
            data: pdu.to_bytes()?,
        },
    })
}

/// Extracts one AVRCP vendor PDU from either an AV/C command or response frame.
pub fn pdu_from_frame(frame: &bumble_avc::Frame) -> Result<VendorPdu> {
    let body = match frame {
        bumble_avc::Frame::Command { body, .. } | bumble_avc::Frame::Response { body, .. } => body,
    };
    match body {
        bumble_avc::FrameBody::VendorDependent { company_id, data } => {
            if *company_id != BLUETOOTH_SIG_COMPANY_ID {
                return Err(Error::WrongCompanyId(*company_id));
            }
            VendorPdu::from_bytes(data)
        }
        _ => Err(Error::NotVendorDependent),
    }
}

/// Wraps an AV/C frame in an AVRCP AVCTP command message.
pub fn avctp_command(
    transaction_label: u8,
    command_type: bumble_avc::CommandType,
    pdu: &VendorPdu,
) -> Result<bumble_avctp::Message> {
    let payload = command_frame(command_type, pdu)?.to_bytes()?;
    Ok(bumble_avctp::Message::command(
        transaction_label,
        AVRCP_PID,
        payload,
    ))
}

/// Extracts an AVRCP PDU from a completely reassembled AVCTP message.
pub fn pdu_from_avctp_message(message: &bumble_avctp::Message) -> Result<VendorPdu> {
    if message.pid != AVRCP_PID {
        return Err(Error::WrongPid(message.pid));
    }
    let frame = bumble_avc::Frame::from_bytes(&message.payload)?;
    pdu_from_frame(&frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(pdu_id: u8, packet_type: u8, parameters: &[u8]) -> Vec<u8> {
        let mut bytes = vec![pdu_id, packet_type];
        bytes.extend_from_slice(&(parameters.len() as u16).to_be_bytes());
        bytes.extend_from_slice(parameters);
        bytes
    }

    #[test]
    fn upstream_pdu_assembler_vectors() {
        let mut assembler = PduAssembler::new();
        assert_eq!(
            assembler.push(&bytes(0x10, 0, &[1])).unwrap(),
            Some((PduId::GET_CAPABILITIES, vec![1]))
        );

        assert_eq!(assembler.push(&bytes(0x10, 1, &[1, 2, 3])).unwrap(), None);
        assert_eq!(
            assembler.push(&bytes(0x10, 0, &[1, 2, 3])).unwrap(),
            Some((PduId::GET_CAPABILITIES, vec![1, 2, 3]))
        );

        assert_eq!(assembler.push(&bytes(0x10, 1, &[1])).unwrap(), None);
        assert_eq!(assembler.push(&bytes(0x10, 2, &[2])).unwrap(), None);
        assert_eq!(
            assembler.push(&bytes(0x10, 3, &[3])).unwrap(),
            Some((PduId::GET_CAPABILITIES, vec![1, 2, 3]))
        );

        assert_eq!(assembler.push(&bytes(0x10, 3, &[1, 2, 3])).unwrap(), None);
    }

    #[test]
    fn malformed_or_mismatched_fragments_are_bounded() {
        let mut assembler = PduAssembler::new();
        assert_eq!(
            assembler.push(&[0x10, 0, 0, 2, 1]),
            Err(Error::LengthMismatch {
                declared: 2,
                actual: 1
            })
        );
        assert_eq!(assembler.push(&bytes(0x10, 1, &[1])).unwrap(), None);
        assert_eq!(assembler.push(&bytes(0x11, 2, &[2])).unwrap(), None);
        assert_eq!(assembler.push(&bytes(0x10, 3, &[3])).unwrap(), None);
    }

    #[test]
    fn outgoing_fragmentation_round_trips() {
        let fragments = fragment_pdu(PduId::GET_CAPABILITIES, &[1, 2, 3, 4, 5], 2).unwrap();
        assert_eq!(
            fragments
                .iter()
                .map(|pdu| pdu.packet_type)
                .collect::<Vec<_>>(),
            vec![PacketType::Start, PacketType::Continue, PacketType::End]
        );
        let mut assembler = PduAssembler::new();
        let mut complete = None;
        for fragment in fragments {
            complete = assembler
                .push(&fragment.to_bytes().unwrap())
                .unwrap()
                .or(complete);
        }
        assert_eq!(
            complete,
            Some((PduId::GET_CAPABILITIES, vec![1, 2, 3, 4, 5]))
        );
    }

    #[test]
    fn avc_vendor_envelope_is_byte_exact() {
        let pdu = VendorPdu {
            pdu_id: PduId::GET_CAPABILITIES,
            packet_type: PacketType::Single,
            parameters: vec![1],
        };
        let frame = command_frame(bumble_avc::CommandType::STATUS, &pdu).unwrap();
        assert_eq!(frame.to_bytes().unwrap(), hex("0148000019581000000101"));
        let parsed = bumble_avc::Frame::from_bytes(&frame.to_bytes().unwrap()).unwrap();
        assert_eq!(pdu_from_frame(&parsed).unwrap(), pdu);
    }

    #[test]
    fn avctp_command_stack_is_byte_exact() {
        let pdu = VendorPdu {
            pdu_id: PduId::GET_CAPABILITIES,
            packet_type: PacketType::Single,
            parameters: vec![1],
        };
        let message = avctp_command(7, bumble_avc::CommandType::STATUS, &pdu).unwrap();
        assert_eq!(
            message.encode_pdus(64).unwrap(),
            vec![hex("70110e0148000019581000000101")]
        );
        assert_eq!(pdu_from_avctp_message(&message).unwrap(), pdu);
    }

    fn hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = (pair[0] as char).to_digit(16).unwrap() as u8;
                let low = (pair[1] as char).to_digit(16).unwrap() as u8;
                (high << 4) | low
            })
            .collect()
    }
}
