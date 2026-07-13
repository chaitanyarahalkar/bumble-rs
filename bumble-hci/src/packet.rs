//! HCI data packets: ACL (5.4.2), Synchronous/SCO (5.4.3), ISO (5.4.5), and
//! the custom passthrough packet. Ported from the corresponding
//! `bumble.hci.HCI_*DataPacket` classes.

use crate::codes::*;
use crate::{Error, Reader, Result};

/// An HCI ACL data packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclDataPacket {
    pub connection_handle: u16,
    pub pb_flag: u8,
    pub bc_flag: u8,
    pub data_total_length: u16,
    pub data: Vec<u8>,
}

pub const HCI_ACL_PB_FIRST_NON_FLUSHABLE: u8 = 0b00;
pub const HCI_ACL_PB_CONTINUATION: u8 = 0b01;
pub const HCI_ACL_PB_FIRST_FLUSHABLE: u8 = 0b10;

/// Split one complete L2CAP PDU into controller-buffer-sized ACL packets.
pub fn fragment_l2cap_pdu(
    connection_handle: u16,
    bc_flag: u8,
    max_fragment_size: usize,
    pdu: &[u8],
    flushable: bool,
) -> Result<Vec<AclDataPacket>> {
    if max_fragment_size == 0 || max_fragment_size > u16::MAX as usize {
        return Err(Error::InvalidPacket(
            "ACL fragment size must be between 1 and 65535".into(),
        ));
    }
    if pdu.len() < 4 {
        return Err(Error::InvalidPacket("truncated L2CAP PDU".into()));
    }
    let declared = usize::from(u16::from_le_bytes([pdu[0], pdu[1]])) + 4;
    if declared != pdu.len() {
        return Err(Error::InvalidPacket(format!(
            "L2CAP PDU length {} != {}",
            pdu.len(),
            declared
        )));
    }
    Ok(pdu
        .chunks(max_fragment_size)
        .enumerate()
        .map(|(index, data)| AclDataPacket {
            connection_handle,
            pb_flag: if index == 0 {
                if flushable {
                    HCI_ACL_PB_FIRST_FLUSHABLE
                } else {
                    HCI_ACL_PB_FIRST_NON_FLUSHABLE
                }
            } else {
                HCI_ACL_PB_CONTINUATION
            },
            bc_flag,
            data_total_length: data.len() as u16,
            data: data.to_vec(),
        })
        .collect())
}

/// Reassembles ACL fragments into complete L2CAP PDUs for one connection.
#[derive(Clone, Debug, Default)]
pub struct AclDataPacketAssembler {
    connection_handle: Option<u16>,
    current_data: Vec<u8>,
    expected_length: Option<usize>,
}

impl AclDataPacketAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, packet: &AclDataPacket) -> Result<Option<Vec<u8>>> {
        if usize::from(packet.data_total_length) != packet.data.len() {
            return Err(Error::InvalidPacket(
                "ACL packet data_total_length mismatch".into(),
            ));
        }
        match packet.pb_flag {
            HCI_ACL_PB_FIRST_NON_FLUSHABLE | HCI_ACL_PB_FIRST_FLUSHABLE => {
                if packet.data.len() < 2 {
                    self.reset();
                    return Err(Error::InvalidPacket(
                        "first ACL fragment lacks L2CAP length".into(),
                    ));
                }
                self.connection_handle = Some(packet.connection_handle);
                self.expected_length =
                    Some(usize::from(u16::from_le_bytes([packet.data[0], packet.data[1]])) + 4);
                self.current_data.clone_from(&packet.data);
            }
            HCI_ACL_PB_CONTINUATION => {
                if self.expected_length.is_none() {
                    return Err(Error::InvalidPacket(
                        "ACL continuation without a start".into(),
                    ));
                }
                if self.connection_handle != Some(packet.connection_handle) {
                    self.reset();
                    return Err(Error::InvalidPacket(
                        "ACL continuation changed connection handle".into(),
                    ));
                }
                self.current_data.extend_from_slice(&packet.data);
            }
            other => {
                self.reset();
                return Err(Error::InvalidPacket(format!(
                    "invalid ACL packet boundary flag {other}"
                )));
            }
        }

        let expected = self.expected_length.expect("set by start or continuation");
        if self.current_data.len() < expected {
            return Ok(None);
        }
        if self.current_data.len() > expected {
            self.reset();
            return Err(Error::InvalidPacket(
                "ACL data exceeds declared L2CAP PDU length".into(),
            ));
        }
        let complete = core::mem::take(&mut self.current_data);
        self.connection_handle = None;
        self.expected_length = None;
        Ok(Some(complete))
    }

    pub fn reset(&mut self) {
        self.connection_handle = None;
        self.current_data.clear();
        self.expected_length = None;
    }

    pub fn is_assembling(&self) -> bool {
        self.expected_length.is_some()
    }
}

impl AclDataPacket {
    pub fn from_bytes(packet: &[u8]) -> Result<AclDataPacket> {
        let mut r = Reader::new(packet, 1);
        let h = r.u16_le()?;
        let data_total_length = r.u16_le()?;
        let data = r.rest().to_vec();
        if data.len() != data_total_length as usize {
            return Err(Error::InvalidPacket(format!(
                "invalid packet length {} != {}",
                data.len(),
                data_total_length
            )));
        }
        Ok(AclDataPacket {
            connection_handle: h & 0x0FFF,
            pb_flag: ((h >> 12) & 0b11) as u8,
            bc_flag: ((h >> 14) & 0b11) as u8,
            data_total_length,
            data,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let h =
            ((self.pb_flag as u16) << 12) | ((self.bc_flag as u16) << 14) | self.connection_handle;
        let mut out = Vec::with_capacity(5 + self.data.len());
        out.push(HCI_ACL_DATA_PACKET);
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&self.data_total_length.to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// An HCI Synchronous (SCO) data packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynchronousDataPacket {
    pub connection_handle: u16,
    /// Packet status flag (2 bits).
    pub packet_status: u8,
    pub data_total_length: u8,
    pub data: Vec<u8>,
}

impl SynchronousDataPacket {
    pub fn from_bytes(packet: &[u8]) -> Result<SynchronousDataPacket> {
        let mut r = Reader::new(packet, 1);
        let h = r.u16_le()?;
        let data_total_length = r.u8()?;
        let data = r.rest().to_vec();
        if data.len() != data_total_length as usize {
            return Err(Error::InvalidPacket(format!(
                "invalid packet length {} != {}",
                data.len(),
                data_total_length
            )));
        }
        Ok(SynchronousDataPacket {
            connection_handle: h & 0x0FFF,
            packet_status: ((h >> 12) & 0b11) as u8,
            data_total_length,
            data,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let h = ((self.packet_status as u16) << 12) | self.connection_handle;
        let mut out = Vec::with_capacity(4 + self.data.len());
        out.push(HCI_SYNCHRONOUS_DATA_PACKET);
        out.extend_from_slice(&h.to_le_bytes());
        out.push(self.data_total_length);
        out.extend_from_slice(&self.data);
        out
    }
}

/// An HCI ISO data packet (5.4.5). The timestamp and SDU-info blocks are
/// present or absent depending on `ts_flag` and `pb_flag`, matching Bumble.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoDataPacket {
    pub connection_handle: u16,
    pub pb_flag: u8,
    pub ts_flag: u8,
    pub data_total_length: u16,
    pub time_stamp: Option<u32>,
    pub packet_sequence_number: Option<u16>,
    pub iso_sdu_length: Option<u16>,
    pub packet_status_flag: Option<u8>,
    pub iso_sdu_fragment: Vec<u8>,
}

impl IsoDataPacket {
    pub fn from_bytes(packet: &[u8]) -> Result<IsoDataPacket> {
        let mut r = Reader::new(packet, 1);
        let pdu_info = r.u16_le()?;
        let data_total_length = r.u16_le()?;
        let connection_handle = pdu_info & 0x0FFF;
        let pb_flag = ((pdu_info >> 12) & 0b11) as u8;
        let ts_flag = ((pdu_info >> 14) & 0b01) as u8;

        // SDU info is present for the first/complete fragment (pb_flag bit 0 clear).
        let should_include_sdu_info = (pb_flag & 0b01) == 0;

        let time_stamp = if ts_flag != 0 {
            Some(r.u32_le()?)
        } else {
            None
        };

        let (packet_sequence_number, iso_sdu_length, packet_status_flag) =
            if should_include_sdu_info {
                let psn = r.u16_le()?;
                let sdu_info = r.u16_le()?;
                (
                    Some(psn),
                    Some(sdu_info & 0x0FFF),
                    Some(((sdu_info >> 15) & 1) as u8),
                )
            } else {
                (None, None, None)
            };

        Ok(IsoDataPacket {
            connection_handle,
            pb_flag,
            ts_flag,
            data_total_length,
            time_stamp,
            packet_sequence_number,
            iso_sdu_length,
            packet_status_flag,
            iso_sdu_fragment: r.rest().to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let pdu_info =
            ((self.ts_flag as u16) << 14) | ((self.pb_flag as u16) << 12) | self.connection_handle;
        let mut out = Vec::new();
        out.push(HCI_ISO_DATA_PACKET);
        out.extend_from_slice(&pdu_info.to_le_bytes());
        out.extend_from_slice(&self.data_total_length.to_le_bytes());
        if let Some(ts) = self.time_stamp {
            out.extend_from_slice(&ts.to_le_bytes());
        }
        if let (Some(psn), Some(sdu_len), Some(status)) = (
            self.packet_sequence_number,
            self.iso_sdu_length,
            self.packet_status_flag,
        ) {
            out.extend_from_slice(&psn.to_le_bytes());
            out.extend_from_slice(&(sdu_len | ((status as u16) << 15)).to_le_bytes());
        }
        out.extend_from_slice(&self.iso_sdu_fragment);
        out
    }
}

/// A passthrough packet for any leading type byte not recognized as a standard
/// HCI packet. Preserves the raw payload verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomPacket {
    payload: Vec<u8>,
}

impl CustomPacket {
    pub fn new(payload: Vec<u8>) -> CustomPacket {
        CustomPacket { payload }
    }

    /// The first byte of the payload (the custom packet type indicator).
    pub fn hci_packet_type(&self) -> u8 {
        self.payload[0]
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.payload.clone()
    }
}
