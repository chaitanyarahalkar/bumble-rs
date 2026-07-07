#!/usr/bin/env python3
"""Build event_embed.json: the hand-written fragments the event generator keeps
verbatim (Command_Complete with typed ReturnParameters, the advertising-report
structs + variants, the LeMeta container, and parse/serialize scaffolding)."""
import os, json

EMB={}
EMB["head"]='''//! HCI Event packets (Vol 2, Part E - 5.4.4), including LE Meta events.
//!
//! Wire form: `[0x04, event_code, param_len, parameters…]`. For LE Meta events
//! (`event_code == 0x3E`) the parameters begin with a sub-event code byte.
//!
//! The `Event` / `LeMetaEvent` enums are GENERATED from upstream `bumble.hci`
//! (see `tools/hcigen`). `Command_Complete` (typed return parameters) and the
//! two advertising-report events (nested report objects) are hand-written and
//! embedded by the generator.

use crate::codes::*;
use crate::return_parameters::ReturnParameters;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// An HCI event. Typed variants carry decoded fields; [`Event::Generic`]
/// preserves raw parameters for event codes with no typed model.'''

EMB["event_variants"]='''    CommandComplete {
        num_hci_command_packets: u8,
        command_opcode: u16,
        return_parameters: ReturnParameters,
    },
    LeMeta(LeMetaEvent),
    /// Any event with no typed model: raw event code + parameters.
    Generic {
        event_code: u8,
        parameters: Vec<u8>,
    },'''

EMB["structs"]='''/// One entry in an LE Advertising Report event.
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

/// An LE Meta sub-event.'''

EMB["meta_variants"]='''    AdvertisingReport {
        reports: Vec<AdvertisingReport>,
    },
    ExtendedAdvertisingReport {
        reports: Vec<ExtendedAdvertisingReport>,
    },
    /// Any LE sub-event with no typed model.
    Generic {
        subevent_code: u8,
        parameters: Vec<u8>,
    },'''

EMB["event_code_arms"]='''            Event::CommandComplete { .. } => HCI_COMMAND_COMPLETE_EVENT,
            Event::LeMeta(_) => HCI_LE_META_EVENT,
            Event::Generic { event_code, .. } => *event_code,'''

EMB["event_params_arms"]='''            Event::CommandComplete {
                num_hci_command_packets,
                command_opcode,
                return_parameters,
            } => {
                p.push(*num_hci_command_packets);
                p.extend_from_slice(&command_opcode.to_le_bytes());
                p.extend_from_slice(&return_parameters.to_bytes());
            }
            Event::LeMeta(m) => p.extend_from_slice(&m.parameters()),
            Event::Generic { parameters, .. } => p.extend_from_slice(parameters),'''

EMB["event_tail"]='''    /// Serialize to the full wire packet.
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
    #[allow(clippy::redundant_closure_call)]
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
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };
        let mut r = Reader::new(parameters, 0);
        let _ = (&addr, &r);
        Ok(match event_code {'''

EMB["from_code_tail"]='''            HCI_COMMAND_COMPLETE_EVENT => {
                let num_hci_command_packets = r.u8()?;
                let command_opcode = r.u16_le()?;
                let return_parameters = ReturnParameters::parse(command_opcode, r.rest())?;
                Event::CommandComplete {
                    num_hci_command_packets,
                    command_opcode,
                    return_parameters,
                }
            }
            _ => Event::Generic {
                event_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}
'''

EMB["meta_subcode_arms"]='''            LeMetaEvent::AdvertisingReport { .. } => HCI_LE_ADVERTISING_REPORT_EVENT,
            LeMetaEvent::ExtendedAdvertisingReport { .. } => {
                HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT
            }
            LeMetaEvent::Generic { subevent_code, .. } => *subevent_code,'''

EMB["meta_params_arms"]='''            LeMetaEvent::AdvertisingReport { reports } => {
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
            }'''

EMB["from_subevent_head"]='''    /// Parse an LE sub-event from its sub-event code and field bytes (the bytes
    /// after the sub-event code).
    #[allow(clippy::redundant_closure_call)]
    pub fn from_subevent(subevent_code: u8, fields: &[u8]) -> Result<LeMetaEvent> {
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };
        let mut r = Reader::new(fields, 0);
        let _ = (&addr, &r);
        Ok(match subevent_code {'''

EMB["from_subevent_tail"]='''            HCI_LE_ADVERTISING_REPORT_EVENT => {
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
'''

json.dump(EMB, open(os.path.dirname(os.path.abspath(__file__))+"/event_embed.json","w"), indent=1)
print("wrote event_embed.json")
