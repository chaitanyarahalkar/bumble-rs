//! AV/C Digital Interface command and response frame codec.

use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
    OperandTooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub u8);
        impl $name { $(pub const $constant: Self = Self($value);)+ }
    };
}

open_u8!(SubunitType {
    MONITOR = 0x00,
    AUDIO = 0x01,
    PRINTER = 0x02,
    DISC = 0x03,
    TAPE_RECORDER_OR_PLAYER = 0x04,
    TUNER = 0x05,
    CA = 0x06,
    CAMERA = 0x07,
    PANEL = 0x09,
    BULLETIN_BOARD = 0x0A,
    VENDOR_UNIQUE = 0x1C,
    EXTENDED = 0x1E,
    UNIT = 0x1F,
});

open_u8!(OperationCode {
    VENDOR_DEPENDENT = 0x00,
    RESERVE = 0x01,
    PLUG_INFO = 0x02,
    DIGITAL_OUTPUT = 0x10,
    DIGITAL_INPUT = 0x11,
    CHANNEL_USAGE = 0x12,
    OUTPUT_PLUG_SIGNAL_FORMAT = 0x18,
    INPUT_PLUG_SIGNAL_FORMAT = 0x19,
    GENERAL_BUS_SETUP = 0x1F,
    CONNECT_AV = 0x20,
    DISCONNECT_AV = 0x21,
    CONNECTIONS = 0x22,
    CONNECT = 0x24,
    DISCONNECT = 0x25,
    UNIT_INFO = 0x30,
    SUBUNIT_INFO = 0x31,
    PASS_THROUGH = 0x7C,
    GUI_UPDATE = 0x7D,
    PUSH_GUI_DATA = 0x7E,
    USER_ACTION = 0x7F,
    VERSION = 0xB0,
    POWER = 0xB2,
});

open_u8!(CommandType {
    CONTROL = 0x00,
    STATUS = 0x01,
    SPECIFIC_INQUIRY = 0x02,
    NOTIFY = 0x03,
    GENERAL_INQUIRY = 0x04,
});

open_u8!(ResponseCode {
    NOT_IMPLEMENTED = 0x08,
    ACCEPTED = 0x09,
    REJECTED = 0x0A,
    IN_TRANSITION = 0x0B,
    IMPLEMENTED_OR_STABLE = 0x0C,
    CHANGED = 0x0D,
    INTERIM = 0x0F,
});

open_u8!(OperationId {
    SELECT = 0x00,
    UP = 0x01,
    DOWN = 0x02,
    LEFT = 0x03,
    RIGHT = 0x04,
    ROOT_MENU = 0x09,
    SETUP_MENU = 0x0A,
    CONTENTS_MENU = 0x0B,
    FAVORITE_MENU = 0x0C,
    EXIT = 0x0D,
    NUMBER_0 = 0x20,
    NUMBER_1 = 0x21,
    NUMBER_2 = 0x22,
    NUMBER_3 = 0x23,
    NUMBER_4 = 0x24,
    NUMBER_5 = 0x25,
    NUMBER_6 = 0x26,
    NUMBER_7 = 0x27,
    NUMBER_8 = 0x28,
    NUMBER_9 = 0x29,
    CHANNEL_UP = 0x30,
    CHANNEL_DOWN = 0x31,
    POWER = 0x40,
    VOLUME_UP = 0x41,
    VOLUME_DOWN = 0x42,
    MUTE = 0x43,
    PLAY = 0x44,
    STOP = 0x45,
    PAUSE = 0x46,
    RECORD = 0x47,
    REWIND = 0x48,
    FAST_FORWARD = 0x49,
    EJECT = 0x4A,
    FORWARD = 0x4B,
    BACKWARD = 0x4C,
    VENDOR_UNIQUE = 0x7E,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateFlag {
    Pressed,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameBody {
    Raw {
        opcode: OperationCode,
        operands: Vec<u8>,
    },
    VendorDependent {
        company_id: u32,
        data: Vec<u8>,
    },
    PassThrough {
        state: StateFlag,
        operation_id: OperationId,
        data: Vec<u8>,
    },
}

impl FrameBody {
    pub fn opcode(&self) -> OperationCode {
        match self {
            Self::Raw { opcode, .. } => *opcode,
            Self::VendorDependent { .. } => OperationCode::VENDOR_DEPENDENT,
            Self::PassThrough { .. } => OperationCode::PASS_THROUGH,
        }
    }

    fn operands(&self) -> Result<Vec<u8>> {
        match self {
            Self::Raw { operands, .. } => Ok(operands.clone()),
            Self::VendorDependent { company_id, data } => {
                if *company_id > 0xFF_FFFF {
                    return Err(Error::Invalid("AV/C company ID"));
                }
                let bytes = company_id.to_be_bytes();
                let mut operands = bytes[1..].to_vec();
                operands.extend_from_slice(data);
                Ok(operands)
            }
            Self::PassThrough {
                state,
                operation_id,
                data,
            } => {
                let length = u8::try_from(data.len()).map_err(|_| Error::OperandTooLong)?;
                let state = match state {
                    StateFlag::Pressed => 0,
                    StateFlag::Released => 0x80,
                };
                let mut operands = vec![state | (operation_id.0 & 0x7F), length];
                operands.extend_from_slice(data);
                Ok(operands)
            }
        }
    }

    fn parse(opcode: OperationCode, operands: &[u8]) -> Result<Self> {
        if opcode == OperationCode::VENDOR_DEPENDENT {
            if operands.len() < 3 {
                return Err(Error::Truncated("vendor-dependent company ID"));
            }
            return Ok(Self::VendorDependent {
                company_id: u32::from_be_bytes([0, operands[0], operands[1], operands[2]]),
                data: operands[3..].to_vec(),
            });
        }
        if opcode == OperationCode::PASS_THROUGH {
            if operands.len() < 2 {
                return Err(Error::Truncated("pass-through operands"));
            }
            let length = usize::from(operands[1]);
            let data = operands
                .get(2..2 + length)
                .ok_or(Error::Truncated("pass-through operation data"))?;
            return Ok(Self::PassThrough {
                state: if operands[0] & 0x80 == 0 {
                    StateFlag::Pressed
                } else {
                    StateFlag::Released
                },
                operation_id: OperationId(operands[0] & 0x7F),
                data: data.to_vec(),
            });
        }
        Ok(Self::Raw {
            opcode,
            operands: operands.to_vec(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Command {
        command_type: CommandType,
        subunit_type: SubunitType,
        subunit_id: u16,
        body: FrameBody,
    },
    Response {
        response_code: ResponseCode,
        subunit_type: SubunitType,
        subunit_id: u16,
        body: FrameBody,
    },
}

impl Frame {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 3 {
            return Err(Error::Truncated("AV/C frame"));
        }
        if data[0] >> 4 != 0 {
            return Err(Error::Invalid("AV/C reserved header bits"));
        }
        let category = data[0] & 0x0F;
        let subunit_type = SubunitType(data[1] >> 3);
        if subunit_type == SubunitType::EXTENDED {
            return Err(Error::Invalid("extended subunit type unsupported"));
        }
        let encoded_id = data[1] & 7;
        let (subunit_id, opcode_offset) = match encoded_id {
            0..=4 | 7 => (u16::from(encoded_id), 2),
            5 => {
                let extension = *data.get(2).ok_or(Error::Truncated("extended subunit ID"))?;
                match extension {
                    0 => return Err(Error::Invalid("reserved extended subunit ID")),
                    0xFF => {
                        let second = *data
                            .get(3)
                            .ok_or(Error::Truncated("double-extended subunit ID"))?;
                        (259 + u16::from(second), 4)
                    }
                    value => (5 + u16::from(value), 3),
                }
            }
            6 => return Err(Error::Invalid("reserved subunit ID")),
            _ => unreachable!(),
        };
        let opcode = OperationCode(
            *data
                .get(opcode_offset)
                .ok_or(Error::Truncated("AV/C opcode"))?,
        );
        let body = FrameBody::parse(opcode, &data[opcode_offset + 1..])?;
        if category < 8 {
            Ok(Self::Command {
                command_type: CommandType(category),
                subunit_type,
                subunit_id,
                body,
            })
        } else {
            Ok(Self::Response {
                response_code: ResponseCode(category),
                subunit_type,
                subunit_id,
                body,
            })
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let (category, subunit_type, subunit_id, body) = match self {
            Self::Command {
                command_type,
                subunit_type,
                subunit_id,
                body,
            } => (command_type.0, *subunit_type, *subunit_id, body),
            Self::Response {
                response_code,
                subunit_type,
                subunit_id,
                body,
            } => (response_code.0, *subunit_type, *subunit_id, body),
        };
        if category > 0x0F || subunit_type.0 > 0x1F || subunit_type == SubunitType::EXTENDED {
            return Err(Error::Invalid("AV/C header field"));
        }
        let mut bytes = vec![category];
        match subunit_id {
            0..=4 | 7 => bytes.push((subunit_type.0 << 3) | subunit_id as u8),
            5..=259 => {
                bytes.push((subunit_type.0 << 3) | 5);
                bytes.push((subunit_id - 5) as u8);
            }
            260..=514 => {
                bytes.push((subunit_type.0 << 3) | 5);
                bytes.push(0xFF);
                bytes.push((subunit_id - 259) as u8);
            }
            _ => return Err(Error::Invalid("AV/C subunit ID")),
        }
        bytes.push(body.opcode().0);
        bytes.extend(body.operands()?);
        Ok(bytes)
    }

    pub fn pass_through_command(state: StateFlag, operation_id: OperationId) -> Self {
        Self::Command {
            command_type: CommandType::CONTROL,
            subunit_type: SubunitType::PANEL,
            subunit_id: 0,
            body: FrameBody::PassThrough {
                state,
                operation_id,
                data: Vec::new(),
            },
        }
    }
}
