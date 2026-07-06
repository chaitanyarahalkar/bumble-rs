//! HCI Event packets (Vol 2, Part E - 5.4.4), including LE Meta events.
//!
//! Wire form: `[0x04, event_code, param_len, parameters…]`. For LE Meta events
//! (`event_code == 0x3E`) the parameters begin with a sub-event code byte.
//! Ported from `bumble.hci.HCI_Event` / `HCI_LE_Meta_Event`.

use crate::codes::*;
use crate::return_parameters::ReturnParameters;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// An HCI event. Typed variants carry decoded fields; [`Event::Generic`]
/// preserves raw parameters for event codes this slice does not decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    CommandComplete {
        num_hci_command_packets: u8,
        command_opcode: u16,
        return_parameters: ReturnParameters,
    },
    CommandStatus {
        status: u8,
        num_hci_command_packets: u8,
        command_opcode: u16,
    },
    NumberOfCompletedPackets {
        connection_handles: Vec<u16>,
        num_completed_packets: Vec<u16>,
    },
    LeMeta(LeMetaEvent),
    /// Any event not decoded by this slice: raw event code + parameters.
    Generic {
        event_code: u8,
        parameters: Vec<u8>,
    },
}

/// One entry in an LE Advertising Report event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvertisingReport {
    pub event_type: u8,
    pub address_type: u8,
    pub address: Address,
    pub data: Vec<u8>,
    pub rssi: i8,
}

/// One entry in an LE Extended Advertising Report event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedAdvertisingReport {
    pub event_type: u16,
    pub address_type: u8,
    pub address: Address,
    pub primary_phy: u8,
    pub secondary_phy: u8,
    pub advertising_sid: u8,
    pub tx_power: i8,
    pub rssi: i8,
    pub periodic_advertising_interval: u16,
    pub direct_address_type: u8,
    pub direct_address: Address,
    pub data: Vec<u8>,
}

/// An LE Meta sub-event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeMetaEvent {
    ConnectionComplete {
        status: u8,
        connection_handle: u16,
        role: u8,
        peer_address_type: u8,
        peer_address: Address,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
        central_clock_accuracy: u8,
    },
    ConnectionUpdateComplete {
        status: u8,
        connection_handle: u16,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
    },
    ChannelSelectionAlgorithm {
        connection_handle: u16,
        channel_selection_algorithm: u8,
    },
    ReadRemoteFeaturesComplete {
        status: u8,
        connection_handle: u16,
        le_features: [u8; 8],
    },
    AdvertisingReport {
        reports: Vec<AdvertisingReport>,
    },
    ExtendedAdvertisingReport {
        reports: Vec<ExtendedAdvertisingReport>,
    },
    /// Any LE sub-event not decoded by this slice.
    Generic {
        subevent_code: u8,
        parameters: Vec<u8>,
    },
}

impl Event {
    /// The 8-bit event code.
    pub fn event_code(&self) -> u8 {
        match self {
            Event::CommandComplete { .. } => HCI_COMMAND_COMPLETE_EVENT,
            Event::CommandStatus { .. } => HCI_COMMAND_STATUS_EVENT,
            Event::NumberOfCompletedPackets { .. } => HCI_NUMBER_OF_COMPLETED_PACKETS_EVENT,
            Event::LeMeta(_) => HCI_LE_META_EVENT,
            Event::Generic { event_code, .. } => *event_code,
        }
    }

    /// The serialized event parameters (without the packet/event-code header).
    pub fn parameters(&self) -> Vec<u8> {
        match self {
            Event::CommandComplete {
                num_hci_command_packets,
                command_opcode,
                return_parameters,
            } => {
                let mut p = Vec::new();
                p.push(*num_hci_command_packets);
                p.extend_from_slice(&command_opcode.to_le_bytes());
                p.extend_from_slice(&return_parameters.to_bytes());
                p
            }
            Event::CommandStatus {
                status,
                num_hci_command_packets,
                command_opcode,
            } => {
                let mut p = Vec::with_capacity(4);
                p.push(*status);
                p.push(*num_hci_command_packets);
                p.extend_from_slice(&command_opcode.to_le_bytes());
                p
            }
            Event::NumberOfCompletedPackets {
                connection_handles,
                num_completed_packets,
            } => {
                // Layout: count byte, then interleaved (handle, count) pairs.
                let mut p = Vec::new();
                p.push(connection_handles.len() as u8);
                for (h, c) in connection_handles.iter().zip(num_completed_packets) {
                    p.extend_from_slice(&h.to_le_bytes());
                    p.extend_from_slice(&c.to_le_bytes());
                }
                p
            }
            Event::LeMeta(m) => m.parameters(),
            Event::Generic { parameters, .. } => parameters.clone(),
        }
    }

    /// Serialize to the full wire packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let params = self.parameters();
        let mut out = Vec::with_capacity(3 + params.len());
        out.push(HCI_EVENT_PACKET);
        out.push(self.event_code());
        out.push(params.len() as u8);
        out.extend_from_slice(&params);
        out
    }

    /// Parse a complete event packet (including the leading type byte).
    pub fn from_bytes(packet: &[u8]) -> Result<Event> {
        if packet.len() < 3 {
            return Err(Error::InvalidPacket("event packet too short".into()));
        }
        let event_code = packet[1];
        let parameters_length = packet[2] as usize;
        if packet.len() < 3 + parameters_length {
            return Err(Error::InvalidPacket(format!(
                "invalid parameters length: expected {parameters_length}, got {}",
                packet.len() - 3
            )));
        }
        let parameters = &packet[3..3 + parameters_length];
        Event::from_code_and_parameters(event_code, parameters)
    }

    /// Build a typed event from its event code and raw parameters.
    pub fn from_code_and_parameters(event_code: u8, parameters: &[u8]) -> Result<Event> {
        if event_code == HCI_LE_META_EVENT {
            let subevent_code = *parameters
                .first()
                .ok_or_else(|| Error::InvalidPacket("empty LE meta parameters".into()))?;
            return Ok(Event::LeMeta(LeMetaEvent::from_subevent(
                subevent_code,
                &parameters[1..],
            )?));
        }

        let mut r = Reader::new(parameters, 0);
        Ok(match event_code {
            HCI_COMMAND_COMPLETE_EVENT => {
                let num_hci_command_packets = r.u8()?;
                let command_opcode = r.u16_le()?;
                let return_parameters = ReturnParameters::parse(command_opcode, r.rest())?;
                Event::CommandComplete {
                    num_hci_command_packets,
                    command_opcode,
                    return_parameters,
                }
            }
            HCI_COMMAND_STATUS_EVENT => Event::CommandStatus {
                status: r.u8()?,
                num_hci_command_packets: r.u8()?,
                command_opcode: r.u16_le()?,
            },
            HCI_NUMBER_OF_COMPLETED_PACKETS_EVENT => {
                // Layout: count byte, then interleaved (handle, count) pairs.
                let count = r.u8()? as usize;
                let mut connection_handles = Vec::with_capacity(count);
                let mut num_completed_packets = Vec::with_capacity(count);
                for _ in 0..count {
                    connection_handles.push(r.u16_le()?);
                    num_completed_packets.push(r.u16_le()?);
                }
                Event::NumberOfCompletedPackets {
                    connection_handles,
                    num_completed_packets,
                }
            }
            _ => Event::Generic {
                event_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}

impl LeMetaEvent {
    /// The LE sub-event code.
    pub fn subevent_code(&self) -> u8 {
        match self {
            LeMetaEvent::ConnectionComplete { .. } => HCI_LE_CONNECTION_COMPLETE_EVENT,
            LeMetaEvent::ConnectionUpdateComplete { .. } => HCI_LE_CONNECTION_UPDATE_COMPLETE_EVENT,
            LeMetaEvent::ChannelSelectionAlgorithm { .. } => {
                HCI_LE_CHANNEL_SELECTION_ALGORITHM_EVENT
            }
            LeMetaEvent::ReadRemoteFeaturesComplete { .. } => {
                HCI_LE_READ_REMOTE_FEATURES_COMPLETE_EVENT
            }
            LeMetaEvent::AdvertisingReport { .. } => HCI_LE_ADVERTISING_REPORT_EVENT,
            LeMetaEvent::ExtendedAdvertisingReport { .. } => {
                HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT
            }
            LeMetaEvent::Generic { subevent_code, .. } => *subevent_code,
        }
    }

    /// Full LE-meta parameters: sub-event code byte followed by the fields.
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = vec![self.subevent_code()];
        match self {
            LeMetaEvent::ConnectionComplete {
                status,
                connection_handle,
                role,
                peer_address_type,
                peer_address,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
                central_clock_accuracy,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*role);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
                p.push(*central_clock_accuracy);
            }
            LeMetaEvent::ConnectionUpdateComplete {
                status,
                connection_handle,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
            }
            LeMetaEvent::ChannelSelectionAlgorithm {
                connection_handle,
                channel_selection_algorithm,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*channel_selection_algorithm);
            }
            LeMetaEvent::ReadRemoteFeaturesComplete {
                status,
                connection_handle,
                le_features,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(le_features);
            }
            LeMetaEvent::AdvertisingReport { reports } => {
                p.push(reports.len() as u8);
                for r in reports {
                    p.push(r.event_type);
                    p.push(r.address_type);
                    p.extend_from_slice(r.address.address_bytes());
                    p.push(r.data.len() as u8);
                    p.extend_from_slice(&r.data);
                    p.push(r.rssi as u8);
                }
            }
            LeMetaEvent::ExtendedAdvertisingReport { reports } => {
                p.push(reports.len() as u8);
                for r in reports {
                    p.extend_from_slice(&r.event_type.to_le_bytes());
                    p.push(r.address_type);
                    p.extend_from_slice(r.address.address_bytes());
                    p.push(r.primary_phy);
                    p.push(r.secondary_phy);
                    p.push(r.advertising_sid);
                    p.push(r.tx_power as u8);
                    p.push(r.rssi as u8);
                    p.extend_from_slice(&r.periodic_advertising_interval.to_le_bytes());
                    p.push(r.direct_address_type);
                    p.extend_from_slice(r.direct_address.address_bytes());
                    p.push(r.data.len() as u8);
                    p.extend_from_slice(&r.data);
                }
            }
            LeMetaEvent::Generic { parameters, .. } => {
                p.extend_from_slice(parameters);
            }
        }
        p
    }

    /// Parse an LE sub-event from its sub-event code and field bytes (the bytes
    /// after the sub-event code).
    pub fn from_subevent(subevent_code: u8, fields: &[u8]) -> Result<LeMetaEvent> {
        let mut r = Reader::new(fields, 0);
        Ok(match subevent_code {
            HCI_LE_CONNECTION_COMPLETE_EVENT => LeMetaEvent::ConnectionComplete {
                status: r.u8()?,
                connection_handle: r.u16_le()?,
                role: r.u8()?,
                peer_address_type: r.u8()?,
                peer_address: Address::from_bytes(r.array::<6>()?, AddressType::RANDOM_DEVICE),
                connection_interval: r.u16_le()?,
                peripheral_latency: r.u16_le()?,
                supervision_timeout: r.u16_le()?,
                central_clock_accuracy: r.u8()?,
            },
            HCI_LE_CONNECTION_UPDATE_COMPLETE_EVENT => LeMetaEvent::ConnectionUpdateComplete {
                status: r.u8()?,
                connection_handle: r.u16_le()?,
                connection_interval: r.u16_le()?,
                peripheral_latency: r.u16_le()?,
                supervision_timeout: r.u16_le()?,
            },
            HCI_LE_CHANNEL_SELECTION_ALGORITHM_EVENT => LeMetaEvent::ChannelSelectionAlgorithm {
                connection_handle: r.u16_le()?,
                channel_selection_algorithm: r.u8()?,
            },
            HCI_LE_READ_REMOTE_FEATURES_COMPLETE_EVENT => LeMetaEvent::ReadRemoteFeaturesComplete {
                status: r.u8()?,
                connection_handle: r.u16_le()?,
                le_features: r.array::<8>()?,
            },
            HCI_LE_ADVERTISING_REPORT_EVENT => {
                let num_reports = r.u8()? as usize;
                let mut reports = Vec::with_capacity(num_reports);
                for _ in 0..num_reports {
                    let event_type = r.u8()?;
                    let address_type = r.u8()?;
                    let address = Address::from_bytes(r.array::<6>()?, AddressType::RANDOM_DEVICE);
                    let data_length = r.u8()? as usize;
                    let data = r.take(data_length)?.to_vec();
                    let rssi = r.u8()? as i8;
                    reports.push(AdvertisingReport {
                        event_type,
                        address_type,
                        address,
                        data,
                        rssi,
                    });
                }
                LeMetaEvent::AdvertisingReport { reports }
            }
            HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT => {
                let num_reports = r.u8()? as usize;
                let mut reports = Vec::with_capacity(num_reports);
                for _ in 0..num_reports {
                    let event_type = r.u16_le()?;
                    let address_type = r.u8()?;
                    let address = Address::from_bytes(r.array::<6>()?, AddressType::RANDOM_DEVICE);
                    let primary_phy = r.u8()?;
                    let secondary_phy = r.u8()?;
                    let advertising_sid = r.u8()?;
                    let tx_power = r.u8()? as i8;
                    let rssi = r.u8()? as i8;
                    let periodic_advertising_interval = r.u16_le()?;
                    let direct_address_type = r.u8()?;
                    let direct_address =
                        Address::from_bytes(r.array::<6>()?, AddressType::RANDOM_DEVICE);
                    let data_length = r.u8()? as usize;
                    let data = r.take(data_length)?.to_vec();
                    reports.push(ExtendedAdvertisingReport {
                        event_type,
                        address_type,
                        address,
                        primary_phy,
                        secondary_phy,
                        advertising_sid,
                        tx_power,
                        rssi,
                        periodic_advertising_interval,
                        direct_address_type,
                        direct_address,
                        data,
                    });
                }
                LeMetaEvent::ExtendedAdvertisingReport { reports }
            }
            _ => LeMetaEvent::Generic {
                subevent_code,
                parameters: fields.to_vec(),
            },
        })
    }
}
