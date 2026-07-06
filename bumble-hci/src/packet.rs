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
