//! Bluetooth Human Interface Device Profile (HIDP) codec and synchronous
//! host/device dispatch.

use core::fmt;

mod l2cap;

pub use l2cap::*;

pub const HID_CONTROL_PSM: u16 = 0x0011;
pub const HID_INTERRUPT_PSM: u16 = 0x0013;

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u8);
        impl $name { $(pub const $constant: Self = Self($value);)+ }
    };
}

open_u8!(ReportType {
    OTHER_REPORT = 0x00,
    INPUT_REPORT = 0x01,
    OUTPUT_REPORT = 0x02,
    FEATURE_REPORT = 0x03,
});

open_u8!(Handshake {
    SUCCESSFUL = 0x00,
    NOT_READY = 0x01,
    ERR_INVALID_REPORT_ID = 0x02,
    ERR_UNSUPPORTED_REQUEST = 0x03,
    ERR_INVALID_PARAMETER = 0x04,
    ERR_UNKNOWN = 0x0E,
    ERR_FATAL = 0x0F,
});

open_u8!(MessageType {
    HANDSHAKE = 0x00,
    CONTROL = 0x01,
    GET_REPORT = 0x04,
    SET_REPORT = 0x05,
    GET_PROTOCOL = 0x06,
    SET_PROTOCOL = 0x07,
    DATA = 0x0A,
});

open_u8!(ProtocolMode {
    BOOT_PROTOCOL = 0x00,
    REPORT_PROTOCOL = 0x01,
});

impl Default for ProtocolMode {
    fn default() -> Self {
        Self::BOOT_PROTOCOL
    }
}

open_u8!(ControlCommand {
    SUSPEND = 0x03,
    EXIT_SUSPEND = 0x04,
    VIRTUAL_CABLE_UNPLUG = 0x05,
});

open_u8!(GetSetReturn {
    FAILURE = 0x00,
    REPORT_ID_NOT_FOUND = 0x01,
    ERR_UNSUPPORTED_REQUEST = 0x02,
    ERR_UNKNOWN = 0x03,
    ERR_INVALID_PARAMETER = 0x04,
    SUCCESS = 0xFF,
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated(&'static str),
    Invalid(&'static str),
    TrailingBytes(usize),
    ChannelNotOpen(u16),
    L2cap(bumble_l2cap::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl From<bumble_l2cap::Error> for Error {
    fn from(error: bumble_l2cap::Error) -> Self {
        Self::L2cap(error)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Handshake(Handshake),
    Control(ControlCommand),
    GetReport {
        report_type: ReportType,
        report_id: u8,
        buffer_size: Option<u16>,
    },
    SetReport {
        report_type: ReportType,
        data: Vec<u8>,
    },
    GetProtocol,
    SetProtocol(ProtocolMode),
    Data {
        report_type: ReportType,
        data: Vec<u8>,
    },
    Unknown {
        message_type: MessageType,
        parameter: u8,
        data: Vec<u8>,
    },
}

impl Message {
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Handshake(_) => MessageType::HANDSHAKE,
            Self::Control(_) => MessageType::CONTROL,
            Self::GetReport { .. } => MessageType::GET_REPORT,
            Self::SetReport { .. } => MessageType::SET_REPORT,
            Self::GetProtocol => MessageType::GET_PROTOCOL,
            Self::SetProtocol(_) => MessageType::SET_PROTOCOL,
            Self::Data { .. } => MessageType::DATA,
            Self::Unknown { message_type, .. } => *message_type,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let (parameter, data) = match self {
            Self::Handshake(result) => (result.0, Vec::new()),
            Self::Control(command) => (command.0, Vec::new()),
            Self::GetReport {
                report_type,
                report_id,
                buffer_size,
            } => {
                let mut data = vec![*report_id];
                let parameter = if let Some(buffer_size) = buffer_size {
                    data.extend_from_slice(&buffer_size.to_le_bytes());
                    0x08 | report_type.0
                } else {
                    report_type.0
                };
                (parameter, data)
            }
            Self::SetReport { report_type, data } | Self::Data { report_type, data } => {
                (report_type.0, data.clone())
            }
            Self::GetProtocol => (0, Vec::new()),
            Self::SetProtocol(mode) => (mode.0, Vec::new()),
            Self::Unknown {
                parameter, data, ..
            } => (*parameter, data.clone()),
        };
        if self.message_type().0 > 0x0F || parameter > 0x0F {
            return Err(Error::Invalid("HIDP header field"));
        }
        let mut bytes = vec![(self.message_type().0 << 4) | parameter];
        bytes.extend_from_slice(&data);
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (&header, data) = bytes.split_first().ok_or(Error::Truncated("HIDP header"))?;
        let message_type = MessageType(header >> 4);
        let parameter = header & 0x0F;
        let message = match message_type {
            MessageType::HANDSHAKE => {
                exact_empty(data)?;
                Self::Handshake(Handshake(parameter))
            }
            MessageType::CONTROL => {
                exact_empty(data)?;
                Self::Control(ControlCommand(parameter))
            }
            MessageType::GET_REPORT => {
                let report_id = *data.first().ok_or(Error::Truncated("HIDP report ID"))?;
                let buffer_size = if parameter & 0x08 != 0 {
                    let size = data
                        .get(1..3)
                        .ok_or(Error::Truncated("HIDP report buffer size"))?;
                    if data.len() != 3 {
                        return Err(Error::TrailingBytes(data.len().saturating_sub(3)));
                    }
                    Some(u16::from_le_bytes([size[0], size[1]]))
                } else {
                    if data.len() != 1 {
                        return Err(Error::TrailingBytes(data.len().saturating_sub(1)));
                    }
                    None
                };
                Self::GetReport {
                    report_type: ReportType(parameter & 3),
                    report_id,
                    buffer_size,
                }
            }
            MessageType::SET_REPORT => Self::SetReport {
                report_type: ReportType(parameter & 3),
                data: data.to_vec(),
            },
            MessageType::GET_PROTOCOL => {
                exact_empty(data)?;
                Self::GetProtocol
            }
            MessageType::SET_PROTOCOL => {
                exact_empty(data)?;
                Self::SetProtocol(ProtocolMode(parameter & 1))
            }
            MessageType::DATA => Self::Data {
                report_type: ReportType(parameter & 3),
                data: data.to_vec(),
            },
            _ => Self::Unknown {
                message_type,
                parameter,
                data: data.to_vec(),
            },
        };
        Ok(message)
    }
}

fn exact_empty(data: &[u8]) -> Result<()> {
    if data.is_empty() {
        Ok(())
    } else {
        Err(Error::TrailingBytes(data.len()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetSetStatus {
    pub data: Vec<u8>,
    pub status: GetSetReturn,
}

impl GetSetStatus {
    pub fn success(data: Vec<u8>) -> Self {
        Self {
            data,
            status: GetSetReturn::SUCCESS,
        }
    }

    pub fn unsupported() -> Self {
        Self {
            data: Vec::new(),
            status: GetSetReturn::ERR_UNSUPPORTED_REQUEST,
        }
    }
}

pub trait DeviceDelegate {
    fn get_report(
        &mut self,
        _report_id: u8,
        _report_type: ReportType,
        _buffer_size: Option<u16>,
    ) -> GetSetStatus {
        GetSetStatus::unsupported()
    }

    fn set_report(
        &mut self,
        _report_id: u8,
        _report_type: ReportType,
        _report_size: usize,
        _data: &[u8],
    ) -> GetSetStatus {
        GetSetStatus::unsupported()
    }

    fn get_protocol(&mut self) -> GetSetStatus {
        GetSetStatus::unsupported()
    }

    fn set_protocol(&mut self, _mode: ProtocolMode) -> GetSetStatus {
        GetSetStatus::unsupported()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    SendControl(Message),
    ControlData {
        report_type: ReportType,
        data: Vec<u8>,
    },
    InterruptData {
        report_type: ReportType,
        data: Vec<u8>,
    },
    Suspend,
    ExitSuspend,
    VirtualCableUnplug,
    Unsupported(Message),
}

pub struct DeviceRuntime<D> {
    delegate: D,
    control_peer_mtu: usize,
}

impl<D: DeviceDelegate> DeviceRuntime<D> {
    pub fn new(delegate: D, control_peer_mtu: usize) -> Self {
        Self {
            delegate,
            control_peer_mtu,
        }
    }

    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    pub fn delegate_mut(&mut self) -> &mut D {
        &mut self.delegate
    }

    pub fn handle_control(&mut self, bytes: &[u8]) -> Result<Vec<DeviceEvent>> {
        let message = Message::from_bytes(bytes)?;
        let event = match message {
            Message::GetReport {
                report_type,
                report_id,
                buffer_size,
            } => {
                let result = self
                    .delegate
                    .get_report(report_id, report_type, buffer_size);
                let response = match result.status {
                    GetSetReturn::SUCCESS => {
                        let mut data = vec![report_id];
                        data.extend_from_slice(&result.data);
                        if data.len() < self.control_peer_mtu {
                            Message::Data { report_type, data }
                        } else {
                            Message::Handshake(Handshake::ERR_INVALID_PARAMETER)
                        }
                    }
                    GetSetReturn::REPORT_ID_NOT_FOUND => {
                        Message::Handshake(Handshake::ERR_INVALID_REPORT_ID)
                    }
                    GetSetReturn::ERR_INVALID_PARAMETER => {
                        Message::Handshake(Handshake::ERR_INVALID_PARAMETER)
                    }
                    GetSetReturn::ERR_UNSUPPORTED_REQUEST => {
                        Message::Handshake(Handshake::ERR_UNSUPPORTED_REQUEST)
                    }
                    _ => Message::Handshake(Handshake::ERR_UNKNOWN),
                };
                DeviceEvent::SendControl(response)
            }
            Message::SetReport { report_type, data } => {
                let (&report_id, report_data) = data
                    .split_first()
                    .ok_or(Error::Truncated("HIDP set-report ID"))?;
                let result = self.delegate.set_report(
                    report_id,
                    report_type,
                    report_data.len() + 1,
                    report_data,
                );
                let handshake = match result.status {
                    GetSetReturn::SUCCESS => Handshake::SUCCESSFUL,
                    GetSetReturn::ERR_INVALID_PARAMETER => Handshake::ERR_INVALID_PARAMETER,
                    GetSetReturn::REPORT_ID_NOT_FOUND => Handshake::ERR_INVALID_REPORT_ID,
                    _ => Handshake::ERR_UNSUPPORTED_REQUEST,
                };
                DeviceEvent::SendControl(Message::Handshake(handshake))
            }
            Message::GetProtocol => {
                let result = self.delegate.get_protocol();
                DeviceEvent::SendControl(if result.status == GetSetReturn::SUCCESS {
                    Message::Data {
                        report_type: ReportType::OTHER_REPORT,
                        data: result.data,
                    }
                } else {
                    Message::Handshake(Handshake::ERR_UNSUPPORTED_REQUEST)
                })
            }
            Message::SetProtocol(mode) => {
                let result = self.delegate.set_protocol(mode);
                DeviceEvent::SendControl(Message::Handshake(
                    if result.status == GetSetReturn::SUCCESS {
                        Handshake::SUCCESSFUL
                    } else {
                        Handshake::ERR_UNSUPPORTED_REQUEST
                    },
                ))
            }
            Message::Data { report_type, data } => DeviceEvent::ControlData { report_type, data },
            Message::Control(ControlCommand::SUSPEND) => DeviceEvent::Suspend,
            Message::Control(ControlCommand::EXIT_SUSPEND) => DeviceEvent::ExitSuspend,
            Message::Control(ControlCommand::VIRTUAL_CABLE_UNPLUG) => {
                DeviceEvent::VirtualCableUnplug
            }
            message @ Message::Control(_) => DeviceEvent::Unsupported(message),
            message => {
                return Ok(vec![
                    DeviceEvent::Unsupported(message),
                    DeviceEvent::SendControl(Message::Handshake(
                        Handshake::ERR_UNSUPPORTED_REQUEST,
                    )),
                ]);
            }
        };
        Ok(vec![event])
    }

    pub fn handle_interrupt(&mut self, bytes: &[u8]) -> Result<DeviceEvent> {
        let message = Message::from_bytes(bytes)?;
        Ok(match message {
            Message::Data { report_type, data } => DeviceEvent::InterruptData { report_type, data },
            message => DeviceEvent::Unsupported(message),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    Handshake(Handshake),
    ControlData {
        report_type: ReportType,
        data: Vec<u8>,
    },
    InterruptData {
        report_type: ReportType,
        data: Vec<u8>,
    },
    VirtualCableUnplug,
    Unsupported(Message),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HostRuntime;

impl HostRuntime {
    pub fn get_report(report_type: ReportType, report_id: u8, buffer_size: Option<u16>) -> Message {
        Message::GetReport {
            report_type,
            report_id,
            buffer_size,
        }
    }

    pub fn set_report(report_type: ReportType, data: Vec<u8>) -> Message {
        Message::SetReport { report_type, data }
    }

    pub fn get_protocol() -> Message {
        Message::GetProtocol
    }

    pub fn set_protocol(mode: ProtocolMode) -> Message {
        Message::SetProtocol(mode)
    }

    pub fn suspend() -> Message {
        Message::Control(ControlCommand::SUSPEND)
    }

    pub fn exit_suspend() -> Message {
        Message::Control(ControlCommand::EXIT_SUSPEND)
    }

    pub fn virtual_cable_unplug() -> Message {
        Message::Control(ControlCommand::VIRTUAL_CABLE_UNPLUG)
    }

    pub fn send_data(data: Vec<u8>) -> Message {
        Message::Data {
            report_type: ReportType::OUTPUT_REPORT,
            data,
        }
    }

    pub fn handle_control(bytes: &[u8]) -> Result<HostEvent> {
        let message = Message::from_bytes(bytes)?;
        Ok(match message {
            Message::Handshake(handshake) => HostEvent::Handshake(handshake),
            Message::Data { report_type, data } => HostEvent::ControlData { report_type, data },
            Message::Control(ControlCommand::VIRTUAL_CABLE_UNPLUG) => HostEvent::VirtualCableUnplug,
            message => HostEvent::Unsupported(message),
        })
    }

    pub fn handle_interrupt(bytes: &[u8]) -> Result<HostEvent> {
        let message = Message::from_bytes(bytes)?;
        Ok(match message {
            Message::Data { report_type, data } => HostEvent::InterruptData { report_type, data },
            message => HostEvent::Unsupported(message),
        })
    }
}

pub fn device_data(data: Vec<u8>) -> Message {
    Message::Data {
        report_type: ReportType::INPUT_REPORT,
        data,
    }
}
