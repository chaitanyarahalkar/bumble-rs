//! Apple Notification Center Service (ANCS) protocol and GATT runtime.

use crate::{Error, Result};
use bumble::Uuid;
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const ANCS_SERVICE_UUID: &str = "7905F431-B5CE-4E99-A40F-4B1E122D00D0";
pub const ANCS_NOTIFICATION_SOURCE_CHARACTERISTIC_UUID: &str =
    "9FBF120D-6301-42D9-8C58-25E699A21DBD";
pub const ANCS_CONTROL_POINT_CHARACTERISTIC_UUID: &str = "69D1D8F3-45E1-49A8-9821-9BBDFDAAD9D9";
pub const ANCS_DATA_SOURCE_CHARACTERISTIC_UUID: &str = "22EAC6E9-24D6-4BB5-BE44-B36ACE7C7BFB";

pub const DEFAULT_ATTRIBUTE_MAX_LENGTH: u16 = u16::MAX;
const INVALID_ATTRIBUTE_VALUE_LENGTH: u8 = 0x0D;
const UNLIKELY_ERROR: u8 = 0x0E;

pub mod error_code {
    pub const UNKNOWN_COMMAND: u8 = 0xA0;
    pub const INVALID_COMMAND: u8 = 0xA1;
    pub const INVALID_PARAMETER: u8 = 0xA2;
    pub const ACTION_FAILED: u8 = 0xA3;
}

macro_rules! open_u8 {
    ($name:ident { $($constant:ident = $value:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(pub u8);

        impl $name {
            $(pub const $constant: Self = Self($value);)+
        }
    };
}

open_u8!(ActionId {
    POSITIVE = 0,
    NEGATIVE = 1,
});

open_u8!(AppAttributeId {
    DISPLAY_NAME = 0,
});

open_u8!(CategoryId {
    OTHER = 0,
    INCOMING_CALL = 1,
    MISSED_CALL = 2,
    VOICEMAIL = 3,
    SOCIAL = 4,
    SCHEDULE = 5,
    EMAIL = 6,
    NEWS = 7,
    HEALTH_AND_FITNESS = 8,
    BUSINESS_AND_FINANCE = 9,
    LOCATION = 10,
    ENTERTAINMENT = 11,
});

open_u8!(EventId {
    NOTIFICATION_ADDED = 0,
    NOTIFICATION_MODIFIED = 1,
    NOTIFICATION_REMOVED = 2,
});

open_u8!(NotificationAttributeId {
    APP_IDENTIFIER = 0,
    TITLE = 1,
    SUBTITLE = 2,
    MESSAGE = 3,
    MESSAGE_SIZE = 4,
    DATE = 5,
    POSITIVE_ACTION_LABEL = 6,
    NEGATIVE_ACTION_LABEL = 7,
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventFlags(pub u8);

impl EventFlags {
    pub const SILENT: Self = Self(1 << 0);
    pub const IMPORTANT: Self = Self(1 << 1);
    pub const PRE_EXISTING: Self = Self(1 << 2);
    pub const POSITIVE_ACTION: Self = Self(1 << 3);
    pub const NEGATIVE_ACTION: Self = Self(1 << 4);
}

impl core::ops::BitOr for EventFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notification {
    pub event_id: EventId,
    pub event_flags: EventFlags,
    pub category_id: CategoryId,
    pub category_count: u8,
    pub notification_uid: u32,
}

impl Notification {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let bytes: [u8; 8] = data.try_into().map_err(|_| {
            Error::InvalidValue(format!(
                "ANCS notification has length {}, expected 8",
                data.len()
            ))
        })?;
        Ok(Self {
            event_id: EventId(bytes[0]),
            event_flags: EventFlags(bytes[1]),
            category_id: CategoryId(bytes[2]),
            category_count: bytes[3],
            notification_uid: u32::from_le_bytes(bytes[4..8].try_into().expect("four bytes")),
        })
    }

    pub fn to_bytes(self) -> [u8; 8] {
        let mut value = [0; 8];
        value[..4].copy_from_slice(&[
            self.event_id.0,
            self.event_flags.0,
            self.category_id.0,
            self.category_count,
        ]);
        value[4..].copy_from_slice(&self.notification_uid.to_le_bytes());
        value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AncsDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl AncsDate {
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() != 15 || value.as_bytes()[8] != b'T' {
            return Err(Error::InvalidValue(format!(
                "invalid ANCS date {value:?}, expected YYYYMMDDTHHMMSS"
            )));
        }
        Ok(Self {
            year: parse_digits(&value[0..4], "year")?,
            month: parse_digits(&value[4..6], "month")?,
            day: parse_digits(&value[6..8], "day")?,
            hour: parse_digits(&value[9..11], "hour")?,
            minute: parse_digits(&value[11..13], "minute")?,
            second: parse_digits(&value[13..15], "second")?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationAttributeValue {
    Text(String),
    MessageSize(u64),
    Date(AncsDate),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAttribute {
    pub attribute_id: NotificationAttributeId,
    pub value: NotificationAttributeValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppAttribute {
    pub attribute_id: AppAttributeId,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationAttributeRequest {
    pub attribute_id: NotificationAttributeId,
    pub max_length: Option<u16>,
}

impl NotificationAttributeRequest {
    pub fn new(attribute_id: NotificationAttributeId) -> Self {
        let max_length = [
            NotificationAttributeId::TITLE,
            NotificationAttributeId::SUBTITLE,
            NotificationAttributeId::MESSAGE,
        ]
        .contains(&attribute_id)
        .then_some(DEFAULT_ATTRIBUTE_MAX_LENGTH);
        Self {
            attribute_id,
            max_length,
        }
    }

    pub fn with_max_length(attribute_id: NotificationAttributeId, max_length: u16) -> Result<Self> {
        if ![
            NotificationAttributeId::TITLE,
            NotificationAttributeId::SUBTITLE,
            NotificationAttributeId::MESSAGE,
        ]
        .contains(&attribute_id)
        {
            return Err(Error::InvalidValue(format!(
                "ANCS attribute {} does not accept a maximum length",
                attribute_id.0
            )));
        }
        Ok(Self {
            attribute_id,
            max_length: Some(max_length),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AncsCommand {
    GetNotificationAttributes {
        notification_uid: u32,
        attributes: Vec<NotificationAttributeRequest>,
    },
    GetAppAttributes {
        app_identifier: String,
        attributes: Vec<AppAttributeId>,
    },
    PerformNotificationAction {
        notification_uid: u32,
        action: ActionId,
    },
}

impl AncsCommand {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let (&command, parameters) = data
            .split_first()
            .ok_or_else(|| Error::InvalidValue("empty ANCS command".into()))?;
        match command {
            0 => {
                if parameters.len() < 4 {
                    return Err(Error::InvalidValue(
                        "Get Notification Attributes is truncated".into(),
                    ));
                }
                let notification_uid =
                    u32::from_le_bytes(parameters[..4].try_into().expect("four-byte UID"));
                let mut offset = 4;
                let mut attributes = Vec::new();
                while offset < parameters.len() {
                    let attribute_id = NotificationAttributeId(parameters[offset]);
                    offset += 1;
                    let max_length = if [
                        NotificationAttributeId::TITLE,
                        NotificationAttributeId::SUBTITLE,
                        NotificationAttributeId::MESSAGE,
                    ]
                    .contains(&attribute_id)
                    {
                        let bytes = parameters.get(offset..offset + 2).ok_or_else(|| {
                            Error::InvalidValue("ANCS attribute maximum length is truncated".into())
                        })?;
                        offset += 2;
                        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
                    } else {
                        None
                    };
                    attributes.push(NotificationAttributeRequest {
                        attribute_id,
                        max_length,
                    });
                }
                Ok(Self::GetNotificationAttributes {
                    notification_uid,
                    attributes,
                })
            }
            1 => {
                let nul = parameters
                    .iter()
                    .position(|byte| *byte == 0)
                    .ok_or_else(|| {
                        Error::InvalidValue("ANCS app identifier is not NUL terminated".into())
                    })?;
                let app_identifier =
                    String::from_utf8(parameters[..nul].to_vec()).map_err(|error| {
                        Error::InvalidValue(format!("invalid app identifier: {error}"))
                    })?;
                let attributes = parameters[nul + 1..]
                    .iter()
                    .copied()
                    .map(AppAttributeId)
                    .collect();
                Ok(Self::GetAppAttributes {
                    app_identifier,
                    attributes,
                })
            }
            2 if parameters.len() == 5 => Ok(Self::PerformNotificationAction {
                notification_uid: u32::from_le_bytes(
                    parameters[..4].try_into().expect("four-byte UID"),
                ),
                action: ActionId(parameters[4]),
            }),
            _ => Err(Error::InvalidValue(format!(
                "unknown ANCS command {command} or invalid parameter length {}",
                parameters.len()
            ))),
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut value = Vec::new();
        match self {
            Self::GetNotificationAttributes {
                notification_uid,
                attributes,
            } => {
                value.push(0);
                value.extend_from_slice(&notification_uid.to_le_bytes());
                for attribute in attributes {
                    value.push(attribute.attribute_id.0);
                    if let Some(max_length) = attribute.max_length {
                        if ![
                            NotificationAttributeId::TITLE,
                            NotificationAttributeId::SUBTITLE,
                            NotificationAttributeId::MESSAGE,
                        ]
                        .contains(&attribute.attribute_id)
                        {
                            return Err(Error::InvalidValue(format!(
                                "ANCS attribute {} cannot carry a maximum length",
                                attribute.attribute_id.0
                            )));
                        }
                        value.extend_from_slice(&max_length.to_le_bytes());
                    } else if [
                        NotificationAttributeId::TITLE,
                        NotificationAttributeId::SUBTITLE,
                        NotificationAttributeId::MESSAGE,
                    ]
                    .contains(&attribute.attribute_id)
                    {
                        return Err(Error::InvalidValue(format!(
                            "ANCS attribute {} requires a maximum length",
                            attribute.attribute_id.0
                        )));
                    }
                }
            }
            Self::GetAppAttributes {
                app_identifier,
                attributes,
            } => {
                if app_identifier.as_bytes().contains(&0) {
                    return Err(Error::InvalidValue(
                        "ANCS app identifier contains NUL".into(),
                    ));
                }
                value.push(1);
                value.extend_from_slice(app_identifier.as_bytes());
                value.push(0);
                value.extend(attributes.iter().map(|attribute| attribute.0));
            }
            Self::PerformNotificationAction {
                notification_uid,
                action,
            } => {
                value.push(2);
                value.extend_from_slice(&notification_uid.to_le_bytes());
                value.push(action.0);
            }
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AncsResponse {
    NotificationAttributes {
        notification_uid: u32,
        attributes: Vec<NotificationAttribute>,
    },
    AppAttributes {
        app_identifier: String,
        attributes: Vec<AppAttribute>,
    },
}

#[derive(Clone, Debug)]
enum ExpectedResponse {
    Notification {
        notification_uid: u32,
        attribute_count: usize,
    },
    App {
        app_identifier: String,
        attribute_count: usize,
    },
}

#[derive(Clone, Debug)]
pub struct AncsResponseAssembler {
    expected: ExpectedResponse,
    buffer: Vec<u8>,
}

impl AncsResponseAssembler {
    pub fn notification(notification_uid: u32, attribute_count: usize) -> Self {
        Self {
            expected: ExpectedResponse::Notification {
                notification_uid,
                attribute_count,
            },
            buffer: Vec::new(),
        }
    }

    pub fn app(app_identifier: impl Into<String>, attribute_count: usize) -> Self {
        Self {
            expected: ExpectedResponse::App {
                app_identifier: app_identifier.into(),
                attribute_count,
            },
            buffer: Vec::new(),
        }
    }

    pub fn push(&mut self, fragment: &[u8]) -> Result<Option<AncsResponse>> {
        self.buffer.extend_from_slice(fragment);
        match &self.expected {
            ExpectedResponse::Notification {
                notification_uid,
                attribute_count,
            } => parse_notification_response(&self.buffer, *notification_uid, *attribute_count),
            ExpectedResponse::App {
                app_identifier,
                attribute_count,
            } => parse_app_response(&self.buffer, app_identifier, *attribute_count),
        }
    }
}

fn parse_notification_response(
    data: &[u8],
    expected_uid: u32,
    expected_count: usize,
) -> Result<Option<AncsResponse>> {
    if data.is_empty() {
        return Ok(None);
    }
    if data[0] != 0 {
        return Err(Error::InvalidValue(format!(
            "unexpected ANCS response command {}, expected 0",
            data[0]
        )));
    }
    if data.len() < 5 {
        return Ok(None);
    }
    let notification_uid = u32::from_le_bytes(data[1..5].try_into().expect("four-byte UID"));
    if notification_uid != expected_uid {
        return Err(Error::InvalidValue(format!(
            "ANCS response UID {notification_uid} does not match {expected_uid}"
        )));
    }
    let Some((attributes, consumed)) = parse_attribute_tuples(&data[5..], expected_count)? else {
        return Ok(None);
    };
    if 5 + consumed != data.len() {
        return Err(Error::InvalidValue(format!(
            "ANCS notification response has {} trailing bytes",
            data.len() - 5 - consumed
        )));
    }
    let attributes = attributes
        .into_iter()
        .map(|(attribute_id, value)| {
            let attribute_id = NotificationAttributeId(attribute_id);
            let text = String::from_utf8(value)
                .map_err(|error| Error::InvalidValue(format!("invalid ANCS UTF-8: {error}")))?;
            let value = match attribute_id {
                NotificationAttributeId::MESSAGE_SIZE => text
                    .parse()
                    .map(NotificationAttributeValue::MessageSize)
                    .map_err(|error| {
                        Error::InvalidValue(format!("invalid ANCS message size: {error}"))
                    })?,
                NotificationAttributeId::DATE => {
                    NotificationAttributeValue::Date(AncsDate::parse(&text)?)
                }
                _ => NotificationAttributeValue::Text(text),
            };
            Ok(NotificationAttribute {
                attribute_id,
                value,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(AncsResponse::NotificationAttributes {
        notification_uid,
        attributes,
    }))
}

fn parse_app_response(
    data: &[u8],
    expected_identifier: &str,
    expected_count: usize,
) -> Result<Option<AncsResponse>> {
    if data.is_empty() {
        return Ok(None);
    }
    if data[0] != 1 {
        return Err(Error::InvalidValue(format!(
            "unexpected ANCS response command {}, expected 1",
            data[0]
        )));
    }
    let Some(nul) = data[1..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| index + 1)
    else {
        return Ok(None);
    };
    let app_identifier = String::from_utf8(data[1..nul].to_vec())
        .map_err(|error| Error::InvalidValue(format!("invalid app identifier: {error}")))?;
    if app_identifier != expected_identifier {
        return Err(Error::InvalidValue(format!(
            "ANCS app identifier {app_identifier:?} does not match {expected_identifier:?}"
        )));
    }
    let tuple_offset = nul + 1;
    let Some((attributes, consumed)) =
        parse_attribute_tuples(&data[tuple_offset..], expected_count)?
    else {
        return Ok(None);
    };
    if tuple_offset + consumed != data.len() {
        return Err(Error::InvalidValue(format!(
            "ANCS app response has {} trailing bytes",
            data.len() - tuple_offset - consumed
        )));
    }
    let attributes = attributes
        .into_iter()
        .map(|(attribute_id, value)| {
            Ok(AppAttribute {
                attribute_id: AppAttributeId(attribute_id),
                value: String::from_utf8(value).map_err(|error| {
                    Error::InvalidValue(format!("invalid app attribute UTF-8: {error}"))
                })?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(AncsResponse::AppAttributes {
        app_identifier,
        attributes,
    }))
}

type RawAttributes = Vec<(u8, Vec<u8>)>;

fn parse_attribute_tuples(
    data: &[u8],
    expected_count: usize,
) -> Result<Option<(RawAttributes, usize)>> {
    let mut offset = 0;
    let mut attributes = Vec::with_capacity(expected_count);
    while attributes.len() < expected_count {
        if data.len().saturating_sub(offset) < 3 {
            return Ok(None);
        }
        let attribute_id = data[offset];
        let length = usize::from(u16::from_le_bytes([data[offset + 1], data[offset + 2]]));
        let end = offset
            .checked_add(3 + length)
            .ok_or_else(|| Error::InvalidValue("ANCS attribute length overflow".into()))?;
        let Some(value) = data.get(offset + 3..end) else {
            return Ok(None);
        };
        attributes.push((attribute_id, value.to_vec()));
        offset = end;
    }
    Ok(Some((attributes, offset)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingNotification {
    NotificationSource(Vec<u8>),
    DataSource(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
struct AncsState {
    commands: VecDeque<AncsCommand>,
    pending: VecDeque<PendingNotification>,
}

#[derive(Clone, Debug, Default)]
pub struct AncsService {
    state: Arc<Mutex<AncsState>>,
}

impl AncsService {
    pub fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: ancs_service_uuid(),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: notification_source_uuid(),
                    properties: properties::NOTIFY,
                    permissions: permissions::READABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: data_source_uuid(),
                    properties: properties::NOTIFY,
                    permissions: permissions::READABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: control_point_uuid(),
                    properties: properties::WRITE,
                    permissions: permissions::WRITEABLE,
                    value: Vec::new(),
                    descriptors: Vec::new(),
                },
            ],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<AncsHandles> {
        let notification_source = required_handle(server, &notification_source_uuid())?;
        let data_source = required_handle(server, &data_source_uuid())?;
        let control_point = required_handle(server, &control_point_uuid())?;
        let state = Arc::clone(&self.state);
        server.set_dynamic_value(
            control_point,
            DynamicValue::write_only(move |_, value| {
                let command =
                    AncsCommand::from_bytes(value).map_err(|_| INVALID_ATTRIBUTE_VALUE_LENGTH)?;
                state
                    .lock()
                    .map_err(|_| UNLIKELY_ERROR)?
                    .commands
                    .push_back(command);
                Ok(())
            }),
        )?;
        Ok(AncsHandles {
            notification_source,
            data_source,
            control_point,
        })
    }

    pub fn take_command(&self) -> Result<Option<AncsCommand>> {
        self.state
            .lock()
            .map(|mut state| state.commands.pop_front())
            .map_err(|_| Error::InvalidValue("ANCS state lock is poisoned".into()))
    }

    pub fn notify(&self, notification: Notification) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| Error::InvalidValue("ANCS state lock is poisoned".into()))?
            .pending
            .push_back(PendingNotification::NotificationSource(
                notification.to_bytes().to_vec(),
            ));
        Ok(())
    }

    pub fn send_data(&self, data: impl Into<Vec<u8>>) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| Error::InvalidValue("ANCS state lock is poisoned".into()))?
            .pending
            .push_back(PendingNotification::DataSource(data.into()));
        Ok(())
    }

    pub fn take_pending_notifications(&self, handles: AncsHandles) -> Result<Vec<(u16, Vec<u8>)>> {
        let notifications = self
            .state
            .lock()
            .map_err(|_| Error::InvalidValue("ANCS state lock is poisoned".into()))?
            .pending
            .drain(..)
            .map(|notification| match notification {
                PendingNotification::NotificationSource(value) => {
                    (handles.notification_source, value)
                }
                PendingNotification::DataSource(value) => (handles.data_source, value),
            })
            .collect();
        Ok(notifications)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AncsHandles {
    pub notification_source: u16,
    pub data_source: u16,
    pub control_point: u16,
}

#[derive(Clone, Debug)]
pub struct AncsServiceProxy {
    pub service: ServiceProxy,
    pub notification_source: CharacteristicProxy,
    pub data_source: CharacteristicProxy,
    pub control_point: CharacteristicProxy,
}

impl AncsServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            notification_source: required_characteristic(
                characteristics,
                &notification_source_uuid(),
            )?,
            data_source: required_characteristic(characteristics, &data_source_uuid())?,
            control_point: required_characteristic(characteristics, &control_point_uuid())?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let mut services = client.discover_service_by_uuid(transport, &ancs_service_uuid())?;
        let Some(service) = services.drain(..).next() else {
            return Ok(None);
        };
        let characteristics = client.discover_characteristics(transport, &service)?;
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn start(&self, client: &mut GattClient, transport: &mut impl AttTransport) -> Result<()> {
        for characteristic in [&self.notification_source, &self.data_source] {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
                .ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "ANCS notification characteristic {:?} has no CCCD",
                        characteristic.uuid
                    ))
                })?;
            client.subscribe(transport, characteristic.handle, cccd.handle, false)?;
        }
        Ok(())
    }

    pub fn stop(&self, client: &mut GattClient, transport: &mut impl AttTransport) -> Result<()> {
        for characteristic in [&self.notification_source, &self.data_source] {
            let cccd = client
                .discover_descriptors(transport, characteristic)?
                .into_iter()
                .find(|descriptor| descriptor.uuid == Uuid::from_16_bits(0x2902))
                .ok_or_else(|| Error::InvalidValue("ANCS characteristic has no CCCD".into()))?;
            client.unsubscribe(transport, characteristic.handle, cccd.handle)?;
        }
        Ok(())
    }

    pub fn send_command(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        command: &AncsCommand,
    ) -> Result<()> {
        client.write_value(
            transport,
            self.control_point.handle,
            command.to_bytes()?,
            true,
        )?;
        Ok(())
    }

    pub fn notification_from_value(&self, handle: u16, value: &[u8]) -> Result<Notification> {
        if handle != self.notification_source.handle {
            return Err(Error::InvalidValue(format!(
                "notification handle 0x{handle:04X} is not the ANCS Notification Source"
            )));
        }
        Notification::from_bytes(value)
    }

    pub fn is_data_source(&self, handle: u16) -> bool {
        handle == self.data_source.handle
    }
}

#[derive(Clone, Debug, Default)]
pub struct AncsClient {
    started: bool,
    response: Option<AncsResponseAssembler>,
}

impl AncsClient {
    pub fn start(
        &mut self,
        proxy: &AncsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        proxy.start(client, transport)?;
        self.started = true;
        Ok(())
    }

    pub fn stop(
        &mut self,
        proxy: &AncsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<()> {
        proxy.stop(client, transport)?;
        self.started = false;
        self.response = None;
        Ok(())
    }

    pub fn begin_notification_attributes(
        &mut self,
        proxy: &AncsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        notification_uid: u32,
        attributes: Vec<NotificationAttributeRequest>,
    ) -> Result<()> {
        self.require_started()?;
        if self.response.is_some() {
            return Err(Error::InvalidValue(
                "an ANCS command response is already in progress".into(),
            ));
        }
        proxy.send_command(
            client,
            transport,
            &AncsCommand::GetNotificationAttributes {
                notification_uid,
                attributes: attributes.clone(),
            },
        )?;
        self.response = Some(AncsResponseAssembler::notification(
            notification_uid,
            attributes.len(),
        ));
        Ok(())
    }

    pub fn begin_app_attributes(
        &mut self,
        proxy: &AncsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        app_identifier: impl Into<String>,
        attributes: Vec<AppAttributeId>,
    ) -> Result<()> {
        self.require_started()?;
        if self.response.is_some() {
            return Err(Error::InvalidValue(
                "an ANCS command response is already in progress".into(),
            ));
        }
        let app_identifier = app_identifier.into();
        proxy.send_command(
            client,
            transport,
            &AncsCommand::GetAppAttributes {
                app_identifier: app_identifier.clone(),
                attributes: attributes.clone(),
            },
        )?;
        self.response = Some(AncsResponseAssembler::app(app_identifier, attributes.len()));
        Ok(())
    }

    pub fn perform_action(
        &self,
        proxy: &AncsServiceProxy,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        notification_uid: u32,
        action: ActionId,
    ) -> Result<()> {
        self.require_started()?;
        proxy.send_command(
            client,
            transport,
            &AncsCommand::PerformNotificationAction {
                notification_uid,
                action,
            },
        )
    }

    pub fn on_data(&mut self, fragment: &[u8]) -> Result<Option<AncsResponse>> {
        let assembler = self
            .response
            .as_mut()
            .ok_or_else(|| Error::InvalidValue("unexpected ANCS data response".into()))?;
        let response = assembler.push(fragment)?;
        if response.is_some() {
            self.response = None;
        }
        Ok(response)
    }

    fn require_started(&self) -> Result<()> {
        if self.started {
            Ok(())
        } else {
            Err(Error::InvalidValue("ANCS client is not started".into()))
        }
    }
}

fn parse_digits<T>(value: &str, name: &str) -> Result<T>
where
    T: core::str::FromStr,
    T::Err: core::fmt::Display,
{
    value
        .parse()
        .map_err(|error| Error::InvalidValue(format!("invalid ANCS date {name}: {error}")))
}

fn required_handle(server: &GattServer, characteristic_uuid: &Uuid) -> Result<u16> {
    server
        .handles_by_uuid(characteristic_uuid)
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "required ANCS characteristic {:?} is missing",
                characteristic_uuid
            ))
        })
}

fn required_characteristic(
    characteristics: &[CharacteristicProxy],
    characteristic_uuid: &Uuid,
) -> Result<CharacteristicProxy> {
    characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == *characteristic_uuid)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "required ANCS characteristic {:?} is missing",
                characteristic_uuid
            ))
        })
}

fn ancs_service_uuid() -> Uuid {
    Uuid::parse(ANCS_SERVICE_UUID).expect("valid ANCS service UUID")
}

fn notification_source_uuid() -> Uuid {
    Uuid::parse(ANCS_NOTIFICATION_SOURCE_CHARACTERISTIC_UUID).expect("valid ANCS notification UUID")
}

fn control_point_uuid() -> Uuid {
    Uuid::parse(ANCS_CONTROL_POINT_CHARACTERISTIC_UUID).expect("valid ANCS control UUID")
}

fn data_source_uuid() -> Uuid {
    Uuid::parse(ANCS_DATA_SOURCE_CHARACTERISTIC_UUID).expect("valid ANCS data UUID")
}
