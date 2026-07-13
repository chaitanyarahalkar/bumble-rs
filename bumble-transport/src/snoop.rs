use crate::{Error, PacketSink, PacketSource, Result};
use bumble_hci::{HciPacket, HCI_COMMAND_PACKET, HCI_EVENT_PACKET};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BTSNOOP_IDENTIFICATION_PATTERN: &[u8; 8] = b"btsnoop\0";
const BTSNOOP_VERSION: u32 = 1;
const BTSNOOP_TIMESTAMP_DELTA: u64 = 0x00E03AB44A676000;
const UNIX_TO_2000_MICROSECONDS: u64 = 946_684_800_000_000;
const BTSNOOP_UNIX_EPOCH_DELTA: u64 = BTSNOOP_TIMESTAMP_DELTA - UNIX_TO_2000_MICROSECONDS;
const PCAP_MAGIC: u32 = 0xA1B2C3D4;
const PCAP_MAJOR_VERSION: u16 = 2;
const PCAP_MINOR_VERSION: u16 = 4;
const PCAP_SNAPLEN: u32 = 65_535;
const DLT_BLUETOOTH_HCI_H4_WITH_PHDR: u32 = 201;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SnoopDirection {
    HostToController = 0,
    ControllerToHost = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SnoopDataLinkType {
    H1 = 1001,
    H4 = 1002,
    HciBscp = 1003,
    H5 = 1004,
}

impl TryFrom<u32> for SnoopDataLinkType {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            1001 => Ok(Self::H1),
            1002 => Ok(Self::H4),
            1003 => Ok(Self::HciBscp),
            1004 => Ok(Self::H5),
            _ => Err(Error::InvalidSpec(format!(
                "unsupported BTSnoop data link type {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BtSnoopRecord {
    pub original_length: u32,
    pub included_length: u32,
    pub packet_flags: u32,
    pub cumulative_drops: u32,
    pub timestamp: u64,
    packet_bytes: Vec<u8>,
}

impl BtSnoopRecord {
    pub fn direction(&self) -> SnoopDirection {
        if self.packet_flags & 1 == 0 {
            SnoopDirection::HostToController
        } else {
            SnoopDirection::ControllerToHost
        }
    }

    pub fn is_truncated(&self) -> bool {
        self.original_length != self.included_length
    }

    pub fn packet_bytes(&self) -> &[u8] {
        &self.packet_bytes
    }

    pub fn packet(&self) -> Result<Option<HciPacket>> {
        if self.is_truncated() {
            return Ok(None);
        }
        Ok(Some(HciPacket::from_bytes(&self.packet_bytes)?))
    }

    pub fn unix_timestamp_micros(&self) -> Result<u64> {
        self.timestamp
            .checked_sub(BTSNOOP_UNIX_EPOCH_DELTA)
            .ok_or_else(|| Error::InvalidSpec("BTSnoop timestamp predates the Unix epoch".into()))
    }

    pub fn system_time(&self) -> Result<SystemTime> {
        UNIX_EPOCH
            .checked_add(std::time::Duration::from_micros(
                self.unix_timestamp_micros()?,
            ))
            .ok_or_else(|| Error::InvalidSpec("BTSnoop timestamp is out of range".into()))
    }
}

pub struct BtSnoopReader<R> {
    input: R,
    version: u32,
    data_link_type: SnoopDataLinkType,
}

impl<R: Read> BtSnoopReader<R> {
    pub fn new(mut input: R) -> Result<Self> {
        let mut header = [0u8; 16];
        input.read_exact(&mut header)?;
        if &header[..8] != BTSNOOP_IDENTIFICATION_PATTERN {
            return Err(Error::InvalidSpec(
                "not a valid BTSnoop file: unexpected identification pattern".into(),
            ));
        }
        let version = u32::from_be_bytes(header[8..12].try_into().expect("fixed slice"));
        let data_link_type = u32::from_be_bytes(header[12..16].try_into().expect("fixed slice"));
        let data_link_type = SnoopDataLinkType::try_from(data_link_type)?;
        if !matches!(
            data_link_type,
            SnoopDataLinkType::H1 | SnoopDataLinkType::H4
        ) {
            return Err(Error::Unsupported(format!(
                "BTSnoop data link type {}",
                data_link_type as u32
            )));
        }
        Ok(Self {
            input,
            version,
            data_link_type,
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn data_link_type(&self) -> SnoopDataLinkType {
        self.data_link_type
    }

    pub fn read_record(&mut self) -> Result<Option<BtSnoopRecord>> {
        let mut header = [0u8; 24];
        if self.input.read(&mut header[..1])? == 0 {
            return Ok(None);
        }
        self.input.read_exact(&mut header[1..])?;

        let original_length = u32::from_be_bytes(header[0..4].try_into().expect("fixed slice"));
        let included_length = u32::from_be_bytes(header[4..8].try_into().expect("fixed slice"));
        let packet_flags = u32::from_be_bytes(header[8..12].try_into().expect("fixed slice"));
        let cumulative_drops = u32::from_be_bytes(header[12..16].try_into().expect("fixed slice"));
        let timestamp = u64::from_be_bytes(header[16..24].try_into().expect("fixed slice"));
        let included_length_usize =
            usize::try_from(included_length).map_err(|_| Error::PacketTooLarge(usize::MAX))?;
        if included_length_usize > crate::MAX_HCI_PACKET_SIZE {
            return Err(Error::PacketTooLarge(included_length_usize));
        }
        let mut packet_bytes = vec![0u8; included_length_usize];
        self.input.read_exact(&mut packet_bytes)?;

        if self.data_link_type == SnoopDataLinkType::H1 {
            let packet_type = match (packet_flags & 1 != 0, packet_flags & 2 != 0) {
                (true, true) => HCI_EVENT_PACKET,
                (false, true) => HCI_COMMAND_PACKET,
                _ => bumble_hci::HCI_ACL_DATA_PACKET,
            };
            packet_bytes.insert(0, packet_type);
        }

        Ok(Some(BtSnoopRecord {
            original_length,
            included_length,
            packet_flags,
            cumulative_drops,
            timestamp,
            packet_bytes,
        }))
    }

    pub fn into_inner(self) -> R {
        self.input
    }
}

pub trait Snooper {
    fn snoop(&mut self, hci_packet: &[u8], direction: SnoopDirection) -> Result<()>;
}

pub struct BtSnooper<W> {
    output: W,
}

impl<W: Write> BtSnooper<W> {
    pub fn new(mut output: W) -> Result<Self> {
        output.write_all(BTSNOOP_IDENTIFICATION_PATTERN)?;
        output.write_all(&BTSNOOP_VERSION.to_be_bytes())?;
        output.write_all(&(SnoopDataLinkType::H4 as u32).to_be_bytes())?;
        Ok(Self { output })
    }

    pub fn snoop_at(
        &mut self,
        hci_packet: &[u8],
        direction: SnoopDirection,
        timestamp: SystemTime,
    ) -> Result<()> {
        let duration = timestamp
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidSpec("BTSnoop timestamp predates the Unix epoch".into()))?;
        let micros = u64::try_from(duration.as_micros())
            .map_err(|_| Error::InvalidSpec("BTSnoop timestamp is out of range".into()))?;
        self.snoop_at_timestamp(
            hci_packet,
            direction,
            BTSNOOP_UNIX_EPOCH_DELTA.saturating_add(micros),
        )
    }

    pub fn snoop_at_timestamp(
        &mut self,
        hci_packet: &[u8],
        direction: SnoopDirection,
        timestamp: u64,
    ) -> Result<()> {
        let packet_type = *hci_packet
            .first()
            .ok_or_else(|| Error::InvalidSpec("cannot snoop an empty HCI packet".into()))?;
        let length =
            u32::try_from(hci_packet.len()).map_err(|_| Error::PacketTooLarge(hci_packet.len()))?;
        let mut flags = direction as u32;
        if matches!(packet_type, HCI_EVENT_PACKET | HCI_COMMAND_PACKET) {
            flags |= 0x10;
        }
        self.output.write_all(&length.to_be_bytes())?;
        self.output.write_all(&length.to_be_bytes())?;
        self.output.write_all(&flags.to_be_bytes())?;
        self.output.write_all(&0u32.to_be_bytes())?;
        self.output.write_all(&timestamp.to_be_bytes())?;
        self.output.write_all(hci_packet)?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.output
    }
}

impl<W: Write> Snooper for BtSnooper<W> {
    fn snoop(&mut self, hci_packet: &[u8], direction: SnoopDirection) -> Result<()> {
        self.snoop_at(hci_packet, direction, SystemTime::now())
    }
}

pub struct PcapSnooper<W> {
    output: W,
}

impl<W: Write> PcapSnooper<W> {
    pub fn new(mut output: W) -> Result<Self> {
        output.write_all(&PCAP_MAGIC.to_le_bytes())?;
        output.write_all(&PCAP_MAJOR_VERSION.to_le_bytes())?;
        output.write_all(&PCAP_MINOR_VERSION.to_le_bytes())?;
        output.write_all(&0u32.to_le_bytes())?;
        output.write_all(&0u32.to_le_bytes())?;
        output.write_all(&PCAP_SNAPLEN.to_le_bytes())?;
        output.write_all(&DLT_BLUETOOTH_HCI_H4_WITH_PHDR.to_le_bytes())?;
        Ok(Self { output })
    }

    pub fn snoop_at(
        &mut self,
        hci_packet: &[u8],
        direction: SnoopDirection,
        timestamp: SystemTime,
    ) -> Result<()> {
        let duration = timestamp
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidSpec("PCAP timestamp predates the Unix epoch".into()))?;
        let seconds = u32::try_from(duration.as_secs())
            .map_err(|_| Error::InvalidSpec("PCAP timestamp is out of range".into()))?;
        self.snoop_at_timestamp(hci_packet, direction, seconds, duration.subsec_micros())
    }

    pub fn snoop_at_timestamp(
        &mut self,
        hci_packet: &[u8],
        direction: SnoopDirection,
        seconds: u32,
        microseconds: u32,
    ) -> Result<()> {
        let length = hci_packet
            .len()
            .checked_add(4)
            .and_then(|length| u32::try_from(length).ok())
            .ok_or(Error::PacketTooLarge(hci_packet.len()))?;
        self.output.write_all(&seconds.to_le_bytes())?;
        self.output.write_all(&microseconds.to_le_bytes())?;
        self.output.write_all(&length.to_le_bytes())?;
        self.output.write_all(&length.to_le_bytes())?;
        self.output.write_all(&(direction as u32).to_be_bytes())?;
        self.output.write_all(hci_packet)?;
        self.output.flush()?;
        Ok(())
    }

    pub fn into_inner(self) -> W {
        self.output
    }
}

impl<W: Write> Snooper for PcapSnooper<W> {
    fn snoop(&mut self, hci_packet: &[u8], direction: SnoopDirection) -> Result<()> {
        self.snoop_at(hci_packet, direction, SystemTime::now())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnooperFormat {
    BtSnoop,
    Pcap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnooperIoType {
    File,
    Pipe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnooperSpec {
    pub format: SnooperFormat,
    pub io_type: SnooperIoType,
    pub path: PathBuf,
}

impl SnooperSpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let (snooper_type, arguments) = spec
            .split_once(':')
            .ok_or_else(|| Error::InvalidSpec("snooper type prefix missing".into()))?;
        let (io_type, path) = arguments
            .split_once(':')
            .ok_or_else(|| Error::InvalidSpec("snooper I/O type missing".into()))?;
        if path.is_empty() {
            return Err(Error::InvalidSpec("snooper path is empty".into()));
        }
        let format = match snooper_type {
            "btsnoop" => SnooperFormat::BtSnoop,
            "pcapsnoop" => SnooperFormat::Pcap,
            _ => {
                return Err(Error::InvalidSpec(format!(
                    "snooper type {snooper_type} not found"
                )))
            }
        };
        let io_type = match (format, io_type) {
            (_, "file") => SnooperIoType::File,
            (SnooperFormat::Pcap, "pipe") => SnooperIoType::Pipe,
            _ => {
                return Err(Error::InvalidSpec(format!(
                    "I/O type {io_type} not supported"
                )))
            }
        };
        Ok(Self {
            format,
            io_type,
            path: PathBuf::from(path.replace("{pid}", &std::process::id().to_string())),
        })
    }
}

pub enum FileSnooper {
    BtSnoop(BtSnooper<BufWriter<File>>),
    Pcap(PcapSnooper<BufWriter<File>>),
}

impl FileSnooper {
    pub fn open(spec: &str) -> Result<Self> {
        let spec = SnooperSpec::parse(spec)?;
        let file = open_snooper_path(&spec.path, spec.io_type)?;
        let output = BufWriter::new(file);
        match spec.format {
            SnooperFormat::BtSnoop => Ok(Self::BtSnoop(BtSnooper::new(output)?)),
            SnooperFormat::Pcap => Ok(Self::Pcap(PcapSnooper::new(output)?)),
        }
    }
}

fn open_snooper_path(path: &Path, io_type: SnooperIoType) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true);
    if io_type == SnooperIoType::File {
        options.create(true).truncate(true);
    }
    Ok(options.open(path)?)
}

impl Snooper for FileSnooper {
    fn snoop(&mut self, hci_packet: &[u8], direction: SnoopDirection) -> Result<()> {
        match self {
            Self::BtSnoop(snooper) => snooper.snoop(hci_packet, direction),
            Self::Pcap(snooper) => snooper.snoop(hci_packet, direction),
        }
    }
}

pub struct SnoopingTransport<T, S> {
    inner: T,
    snooper: S,
}

impl<T, S> SnoopingTransport<T, S> {
    pub fn new(inner: T, snooper: S) -> Self {
        Self { inner, snooper }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn snooper(&self) -> &S {
        &self.snooper
    }

    pub fn snooper_mut(&mut self) -> &mut S {
        &mut self.snooper
    }

    pub fn into_parts(self) -> (T, S) {
        (self.inner, self.snooper)
    }
}

impl<T: PacketSource, S: Snooper> PacketSource for SnoopingTransport<T, S> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        let packet = self.inner.read_packet()?;
        if let Some(packet) = packet.as_ref() {
            self.snooper
                .snoop(&packet.to_bytes(), SnoopDirection::ControllerToHost)?;
        }
        Ok(packet)
    }
}

impl<T: PacketSink, S: Snooper> PacketSink for SnoopingTransport<T, S> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        self.snooper
            .snoop(&packet.to_bytes(), SnoopDirection::HostToController)?;
        self.inner.write_packet(packet)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}
