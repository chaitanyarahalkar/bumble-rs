use crate::{Error, PacketFramer, PacketSink, PacketSource, Result, MAX_HCI_PACKET_SIZE};
use bumble_hci::{
    HciPacket, HCI_ACL_DATA_PACKET, HCI_COMMAND_PACKET, HCI_EVENT_PACKET, HCI_ISO_DATA_PACKET,
    HCI_SYNCHRONOUS_DATA_PACKET,
};
use core::fmt;
use libusb1_sys::constants::{
    LIBUSB_ERROR_INTERRUPTED, LIBUSB_ERROR_NOT_FOUND, LIBUSB_ERROR_NO_DEVICE, LIBUSB_ERROR_TIMEOUT,
    LIBUSB_SUCCESS, LIBUSB_TRANSFER_CANCELLED, LIBUSB_TRANSFER_COMPLETED,
    LIBUSB_TRANSFER_NO_DEVICE, LIBUSB_TRANSFER_TIMED_OUT,
};
use rusb::{
    Context, Device, DeviceHandle, Direction, Recipient, RequestType, TransferType, UsbContext,
};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const USB_DEVICE_CLASS_DEVICE: u8 = 0x00;
const USB_DEVICE_CLASS_WIRELESS_CONTROLLER: u8 = 0xe0;
const USB_DEVICE_SUBCLASS_RF_CONTROLLER: u8 = 0x01;
const USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER: u8 = 0x01;
const READ_TIMEOUT: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SCO_PACKET_SIZE: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsbSelector {
    Index(usize),
    VidPid {
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        occurrence: usize,
    },
    Path {
        bus: u8,
        ports: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbSpec {
    pub selector: UsbSelector,
    pub forced: bool,
    pub sco_alternate: Option<u8>,
}

impl UsbSpec {
    pub fn parse(spec: &str) -> Result<Self> {
        if spec.is_empty() {
            return Err(Error::InvalidSpec("USB selector is empty".into()));
        }
        let (spec, forced) = match spec.strip_suffix('!') {
            Some(spec) => (spec, true),
            None => (spec, false),
        };
        let (selector, sco_alternate) = match spec.split_once("+sco=") {
            Some((selector, alternate)) => {
                let alternate = alternate.parse::<u8>().map_err(|_| {
                    Error::InvalidSpec(format!("invalid USB SCO alternate {alternate}"))
                })?;
                (selector, Some(alternate))
            }
            None => (spec, None),
        };
        if selector.is_empty() {
            return Err(Error::InvalidSpec("USB selector is empty".into()));
        }

        let selector = if let Some((vendor_id, product)) = selector.split_once(':') {
            let vendor_id = parse_hex_u16(vendor_id, "vendor")?;
            let (product, serial_number, occurrence) =
                if let Some((product, serial)) = product.split_once('/') {
                    if serial.is_empty() {
                        return Err(Error::InvalidSpec("USB serial number is empty".into()));
                    }
                    (product, Some(serial.to_owned()), 0)
                } else if let Some((product, occurrence)) = product.split_once('#') {
                    let occurrence = occurrence.parse::<usize>().map_err(|_| {
                        Error::InvalidSpec(format!("invalid USB occurrence {occurrence}"))
                    })?;
                    (product, None, occurrence)
                } else {
                    (product, None, 0)
                };
            UsbSelector::VidPid {
                vendor_id,
                product_id: parse_hex_u16(product, "product")?,
                serial_number,
                occurrence,
            }
        } else if let Some((bus, ports)) = selector.split_once('-') {
            let bus = bus
                .parse::<u8>()
                .map_err(|_| Error::InvalidSpec(format!("invalid USB bus {bus}")))?;
            let ports = ports
                .split('.')
                .map(|port| {
                    port.parse::<u8>()
                        .map_err(|_| Error::InvalidSpec(format!("invalid USB port {port}")))
                })
                .collect::<Result<Vec<_>>>()?;
            if ports.is_empty() {
                return Err(Error::InvalidSpec("USB port path is empty".into()));
            }
            UsbSelector::Path { bus, ports }
        } else {
            UsbSelector::Index(
                selector
                    .parse::<usize>()
                    .map_err(|_| Error::InvalidSpec(format!("invalid USB index {selector}")))?,
            )
        };
        Ok(Self {
            selector,
            forced,
            sco_alternate,
        })
    }
}

fn parse_hex_u16(value: &str, field: &str) -> Result<u16> {
    u16::from_str_radix(value, 16)
        .map_err(|_| Error::InvalidSpec(format!("invalid USB {field} ID {value}")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbEndpointInfo {
    pub address: u8,
    pub direction: Direction,
    pub transfer_type: TransferType,
    pub max_packet_size: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbInterfaceInfo {
    pub configuration: u8,
    pub interface: u8,
    pub alternate: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: Vec<UsbEndpointInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbInterfaceLayout {
    pub configuration: u8,
    pub interface: u8,
    pub alternate: u8,
    pub interrupt_in: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbScoLayout {
    pub configuration: u8,
    pub interface: u8,
    pub alternate: u8,
    pub isochronous_in: u8,
    pub isochronous_out: u8,
    pub max_packet_size_in: u16,
    pub max_packet_size_out: u16,
}

pub fn select_interface_layout(
    interfaces: &[UsbInterfaceInfo],
    forced: bool,
) -> Option<UsbInterfaceLayout> {
    interfaces.iter().find_map(|interface| {
        if !forced
            && (interface.class, interface.subclass, interface.protocol)
                != (
                    USB_DEVICE_CLASS_WIRELESS_CONTROLLER,
                    USB_DEVICE_SUBCLASS_RF_CONTROLLER,
                    USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER,
                )
        {
            return None;
        }
        let interrupt_in =
            endpoint_address(&interface.endpoints, Direction::In, TransferType::Interrupt)?;
        let bulk_in = endpoint_address(&interface.endpoints, Direction::In, TransferType::Bulk)?;
        let bulk_out = endpoint_address(&interface.endpoints, Direction::Out, TransferType::Bulk)?;
        Some(UsbInterfaceLayout {
            configuration: interface.configuration,
            interface: interface.interface,
            alternate: interface.alternate,
            interrupt_in,
            bulk_in,
            bulk_out,
        })
    })
}

/// Select the SCO/eSCO isochronous setting from the same configuration as the
/// primary HCI interface.
///
/// `alternate == 0` has Bumble's auto-selection meaning: choose the complete
/// setting with the lexicographically largest IN/OUT packet-size pair. A
/// non-zero alternate selects the first complete matching setting.
pub fn select_sco_layout(
    interfaces: &[UsbInterfaceInfo],
    configuration: u8,
    forced: bool,
    alternate: u8,
) -> Option<UsbScoLayout> {
    let mut selected: Option<UsbScoLayout> = None;
    for interface in interfaces {
        if interface.configuration != configuration
            || (!forced
                && (interface.class, interface.subclass, interface.protocol)
                    != (
                        USB_DEVICE_CLASS_WIRELESS_CONTROLLER,
                        USB_DEVICE_SUBCLASS_RF_CONTROLLER,
                        USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER,
                    ))
            || (alternate != 0 && interface.alternate != alternate)
        {
            continue;
        }

        let isochronous_in = largest_endpoint(
            &interface.endpoints,
            Direction::In,
            TransferType::Isochronous,
        );
        let isochronous_out = largest_endpoint(
            &interface.endpoints,
            Direction::Out,
            TransferType::Isochronous,
        );
        let (Some(isochronous_in), Some(isochronous_out)) = (isochronous_in, isochronous_out)
        else {
            continue;
        };
        if isochronous_in.max_packet_size == 0 || isochronous_out.max_packet_size == 0 {
            continue;
        }

        let candidate = UsbScoLayout {
            configuration,
            interface: interface.interface,
            alternate: interface.alternate,
            isochronous_in: isochronous_in.address,
            isochronous_out: isochronous_out.address,
            max_packet_size_in: isochronous_in.max_packet_size,
            max_packet_size_out: isochronous_out.max_packet_size,
        };
        if selected.is_none()
            || (alternate == 0
                && (candidate.max_packet_size_in, candidate.max_packet_size_out)
                    > (
                        selected.expect("checked above").max_packet_size_in,
                        selected.expect("checked above").max_packet_size_out,
                    ))
        {
            selected = Some(candidate);
        }
    }
    selected
}

fn endpoint_address(
    endpoints: &[UsbEndpointInfo],
    direction: Direction,
    transfer_type: TransferType,
) -> Option<u8> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.direction == direction && endpoint.transfer_type == transfer_type)
        .map(|endpoint| endpoint.address)
}

fn largest_endpoint(
    endpoints: &[UsbEndpointInfo],
    direction: Direction,
    transfer_type: TransferType,
) -> Option<UsbEndpointInfo> {
    endpoints
        .iter()
        .filter(|endpoint| {
            endpoint.direction == direction && endpoint.transfer_type == transfer_type
        })
        .max_by_key(|endpoint| endpoint.max_packet_size)
        .copied()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsbTransferError {
    Timeout,
    Disconnected,
    Other(String),
}

impl fmt::Display for UsbTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => formatter.write_str("USB transfer timed out"),
            Self::Disconnected => formatter.write_str("USB device disconnected"),
            Self::Other(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for UsbTransferError {}

pub trait UsbIo {
    fn read_interrupt(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError>;

    fn read_bulk(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError>;

    fn write_control(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        buffer: &[u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError>;

    fn write_bulk(
        &mut self,
        endpoint: u8,
        buffer: &[u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError>;

    fn read_isochronous(
        &mut self,
        _endpoint: u8,
        _max_packet_size: u16,
        _buffer: &mut [u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        Err(UsbTransferError::Other(
            "USB isochronous input is not implemented by this backend".into(),
        ))
    }

    fn write_isochronous(
        &mut self,
        _endpoint: u8,
        _max_packet_size: u16,
        _buffer: &[u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        Err(UsbTransferError::Other(
            "USB isochronous output is not implemented by this backend".into(),
        ))
    }
}

pub struct UsbTransport<B> {
    backend: B,
    layout: UsbInterfaceLayout,
    sco_layout: Option<UsbScoLayout>,
    framer: PacketFramer,
    sco_buffer: Vec<u8>,
    pending: VecDeque<HciPacket>,
    next_poll_endpoint: u8,
    vendor_id: u16,
    product_id: u16,
    bus: u8,
    address: u8,
}

impl<B> UsbTransport<B> {
    pub fn from_backend(
        backend: B,
        layout: UsbInterfaceLayout,
        vendor_id: u16,
        product_id: u16,
        bus: u8,
        address: u8,
    ) -> Self {
        Self::from_backend_with_sco(backend, layout, None, vendor_id, product_id, bus, address)
    }

    pub fn from_backend_with_sco(
        backend: B,
        layout: UsbInterfaceLayout,
        sco_layout: Option<UsbScoLayout>,
        vendor_id: u16,
        product_id: u16,
        bus: u8,
        address: u8,
    ) -> Self {
        Self {
            backend,
            layout,
            sco_layout,
            framer: PacketFramer::new(),
            sco_buffer: Vec::new(),
            pending: VecDeque::new(),
            next_poll_endpoint: 0,
            vendor_id,
            product_id,
            bus,
            address,
        }
    }

    pub fn layout(&self) -> UsbInterfaceLayout {
        self.layout
    }

    pub fn sco_layout(&self) -> Option<UsbScoLayout> {
        self.sco_layout
    }

    pub fn vendor_id(&self) -> u16 {
        self.vendor_id
    }

    pub fn product_id(&self) -> u16 {
        self.product_id
    }

    pub fn bus(&self) -> u8 {
        self.bus
    }

    pub fn address(&self) -> u8 {
        self.address
    }

    pub fn get_ref(&self) -> &B {
        &self.backend
    }

    pub fn get_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

impl<B: UsbIo> UsbTransport<B> {
    fn poll_endpoint(&mut self, events: bool) -> Result<()> {
        let mut transfer = vec![0u8; MAX_HCI_PACKET_SIZE - 1];
        let result = if events {
            self.backend
                .read_interrupt(self.layout.interrupt_in, &mut transfer, READ_TIMEOUT)
        } else {
            self.backend
                .read_bulk(self.layout.bulk_in, &mut transfer, READ_TIMEOUT)
        };
        let count = match result {
            Ok(count) => count,
            Err(UsbTransferError::Timeout) => return Ok(()),
            Err(error) => return Err(transfer_error(error)),
        };
        if count == 0 {
            return Ok(());
        }
        if count > transfer.len() {
            return Err(Error::PacketTooLarge(count + 1));
        }
        let mut framed = Vec::with_capacity(count + 1);
        framed.push(if events {
            HCI_EVENT_PACKET
        } else {
            HCI_ACL_DATA_PACKET
        });
        framed.extend_from_slice(&transfer[..count]);
        self.pending.extend(self.framer.feed(&framed)?);
        Ok(())
    }

    fn poll_sco(&mut self) -> Result<()> {
        let Some(layout) = self.sco_layout else {
            return Ok(());
        };
        let mut transfer = vec![0u8; usize::from(layout.max_packet_size_in)];
        let count = match self.backend.read_isochronous(
            layout.isochronous_in,
            layout.max_packet_size_in,
            &mut transfer,
            READ_TIMEOUT,
        ) {
            Ok(count) => count,
            Err(UsbTransferError::Timeout) => return Ok(()),
            Err(error) => return Err(transfer_error(error)),
        };
        if count == 0 {
            return Ok(());
        }
        if count > transfer.len() {
            return Err(Error::PacketTooLarge(count + 1));
        }
        self.sco_buffer.extend_from_slice(&transfer[..count]);
        while self.sco_buffer.len() >= 3 {
            let packet_size = 3 + usize::from(self.sco_buffer[2]);
            if self.sco_buffer.len() < packet_size {
                break;
            }
            let mut framed = Vec::with_capacity(packet_size + 1);
            framed.push(HCI_SYNCHRONOUS_DATA_PACKET);
            framed.extend(self.sco_buffer.drain(..packet_size));
            self.pending.push_back(HciPacket::from_bytes(&framed)?);
        }
        if self.sco_buffer.len() > MAX_SCO_PACKET_SIZE {
            self.sco_buffer.clear();
            return Err(Error::PacketTooLarge(MAX_SCO_PACKET_SIZE + 1));
        }
        Ok(())
    }

    fn poll_next_endpoint(&mut self) -> Result<()> {
        loop {
            let endpoint = self.next_poll_endpoint;
            self.next_poll_endpoint = (self.next_poll_endpoint + 1) % 3;
            match endpoint {
                0 => return self.poll_endpoint(true),
                1 => return self.poll_endpoint(false),
                2 if self.sco_layout.is_some() => return self.poll_sco(),
                2 => {}
                _ => unreachable!(),
            }
        }
    }
}

impl<B: UsbIo> PacketSource for UsbTransport<B> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }
        loop {
            self.poll_next_endpoint()?;
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
        }
    }
}

impl<B: UsbIo> PacketSink for UsbTransport<B> {
    fn write_packet(&mut self, packet: &HciPacket) -> Result<()> {
        let bytes = packet.to_bytes();
        let (&packet_type, payload) = bytes
            .split_first()
            .ok_or_else(|| Error::InvalidSpec("empty HCI packet".into()))?;
        let count = match packet_type {
            HCI_COMMAND_PACKET => self.backend.write_control(
                rusb::request_type(Direction::Out, RequestType::Class, Recipient::Device),
                0,
                0,
                0,
                payload,
                WRITE_TIMEOUT,
            ),
            HCI_ACL_DATA_PACKET | HCI_ISO_DATA_PACKET => {
                self.backend
                    .write_bulk(self.layout.bulk_out, payload, WRITE_TIMEOUT)
            }
            HCI_SYNCHRONOUS_DATA_PACKET => {
                let layout = self.sco_layout.ok_or_else(|| {
                    Error::Unsupported(
                        "USB SCO output requires a +sco=<alternate> transport selector".into(),
                    )
                })?;
                self.backend.write_isochronous(
                    layout.isochronous_out,
                    layout.max_packet_size_out,
                    payload,
                    WRITE_TIMEOUT,
                )
            }
            other => {
                return Err(Error::InvalidPacketType(other));
            }
        }
        .map_err(transfer_error)?;
        if count != payload.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                format!("partial USB HCI transfer {count}/{}", payload.len()),
            )
            .into());
        }
        Ok(())
    }
}

fn transfer_error(error: UsbTransferError) -> Error {
    let kind = if error == UsbTransferError::Disconnected {
        std::io::ErrorKind::NotConnected
    } else if error == UsbTransferError::Timeout {
        std::io::ErrorKind::TimedOut
    } else {
        std::io::ErrorKind::Other
    };
    std::io::Error::new(kind, error).into()
}

#[derive(Clone)]
pub struct RusbUsbIo {
    handle: Arc<DeviceHandle<Context>>,
    iso_transfer_lock: Arc<Mutex<()>>,
}

impl UsbIo for RusbUsbIo {
    fn read_interrupt(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.handle
            .read_interrupt(endpoint, buffer, timeout)
            .map_err(map_rusb_transfer_error)
    }

    fn read_bulk(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.handle
            .read_bulk(endpoint, buffer, timeout)
            .map_err(map_rusb_transfer_error)
    }

    fn write_control(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        buffer: &[u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.handle
            .write_control(request_type, request, value, index, buffer, timeout)
            .map_err(map_rusb_transfer_error)
    }

    fn write_bulk(
        &mut self,
        endpoint: u8,
        buffer: &[u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.handle
            .write_bulk(endpoint, buffer, timeout)
            .map_err(map_rusb_transfer_error)
    }

    fn read_isochronous(
        &mut self,
        endpoint: u8,
        max_packet_size: u16,
        buffer: &mut [u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        let packet_size = usize::from(max_packet_size).min(buffer.len());
        if packet_size == 0 {
            return Ok(0);
        }
        let _guard = self.iso_transfer_lock.lock().map_err(|_| {
            UsbTransferError::Other("USB isochronous transfer lock is poisoned".into())
        })?;
        let result = run_isochronous_transfer(
            &self.handle,
            endpoint,
            vec![0; packet_size],
            &[packet_size],
            timeout,
        )?;
        let Some(packet) = result.packets.into_iter().next() else {
            return Ok(0);
        };
        buffer[..packet.len()].copy_from_slice(&packet);
        Ok(packet.len())
    }

    fn write_isochronous(
        &mut self,
        endpoint: u8,
        max_packet_size: u16,
        buffer: &[u8],
        timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        let max_packet_size = usize::from(max_packet_size);
        let packet_lengths = isochronous_packet_lengths(buffer.len(), max_packet_size)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        let _guard = self.iso_transfer_lock.lock().map_err(|_| {
            UsbTransferError::Other("USB isochronous transfer lock is poisoned".into())
        })?;
        let result = run_isochronous_transfer(
            &self.handle,
            endpoint,
            buffer.to_vec(),
            &packet_lengths,
            timeout,
        )?;
        Ok(result.transferred)
    }
}

fn isochronous_packet_lengths(
    transfer_length: usize,
    max_packet_size: usize,
) -> core::result::Result<Vec<usize>, UsbTransferError> {
    if max_packet_size == 0 {
        return Err(UsbTransferError::Other(
            "USB isochronous endpoint has a zero packet size".into(),
        ));
    }
    let packet_count = transfer_length.div_ceil(max_packet_size);
    Ok((0..packet_count)
        .map(|index| (transfer_length - index * max_packet_size).min(max_packet_size))
        .collect())
}

fn map_rusb_transfer_error(error: rusb::Error) -> UsbTransferError {
    match error {
        rusb::Error::Timeout => UsbTransferError::Timeout,
        rusb::Error::NoDevice => UsbTransferError::Disconnected,
        error => UsbTransferError::Other(error.to_string()),
    }
}

struct PendingIsoTransfer {
    _handle: Arc<DeviceHandle<Context>>,
    buffer: Vec<u8>,
    done: AtomicBool,
}

struct IsoTransferResult {
    packets: Vec<Vec<u8>>,
    transferred: usize,
}

extern "system" fn iso_transfer_callback(transfer: *mut libusb1_sys::libusb_transfer) {
    if transfer.is_null() {
        return;
    }
    // SAFETY: `run_isochronous_transfer` installs a live boxed
    // `PendingIsoTransfer` as user_data and does not reclaim it until this
    // callback publishes completion with Release ordering.
    let state = unsafe { &*((*transfer).user_data.cast::<PendingIsoTransfer>()) };
    state.done.store(true, Ordering::Release);
}

fn run_isochronous_transfer(
    handle: &Arc<DeviceHandle<Context>>,
    endpoint: u8,
    buffer: Vec<u8>,
    packet_lengths: &[usize],
    timeout: Duration,
) -> core::result::Result<IsoTransferResult, UsbTransferError> {
    if packet_lengths.is_empty() || packet_lengths.iter().sum::<usize>() != buffer.len() {
        return Err(UsbTransferError::Other(
            "invalid USB isochronous packet layout".into(),
        ));
    }
    let packet_count = i32::try_from(packet_lengths.len()).map_err(|_| {
        UsbTransferError::Other("too many USB isochronous packets in one transfer".into())
    })?;
    let transfer_length = i32::try_from(buffer.len())
        .map_err(|_| UsbTransferError::Other("USB isochronous transfer is too large".into()))?;
    let timeout_millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);

    // SAFETY: allocation is checked for null and the transfer is either freed
    // after its callback or deliberately leaked together with its owned state
    // if libusb cannot deliver a cancellation callback.
    let transfer = unsafe { libusb1_sys::libusb_alloc_transfer(packet_count) };
    if transfer.is_null() {
        return Err(UsbTransferError::Other(
            "unable to allocate USB isochronous transfer".into(),
        ));
    }
    let state = Box::new(PendingIsoTransfer {
        _handle: handle.clone(),
        buffer,
        done: AtomicBool::new(false),
    });
    let state = Box::into_raw(state);

    // SAFETY: the boxed state and its Vec allocation remain stable until the
    // transfer callback has run. The allocated transfer has room for exactly
    // `packet_count` descriptors.
    unsafe {
        libusb1_sys::libusb_fill_iso_transfer(
            transfer,
            handle.as_raw(),
            endpoint,
            (*state).buffer.as_mut_ptr(),
            transfer_length,
            packet_count,
            iso_transfer_callback,
            state.cast::<c_void>(),
            timeout_millis,
        );
        for (index, packet_length) in packet_lengths.iter().copied().enumerate() {
            let packet_length = u32::try_from(packet_length)
                .map_err(|_| UsbTransferError::Other("USB isochronous packet is too large".into()));
            let Ok(packet_length) = packet_length else {
                libusb1_sys::libusb_free_transfer(transfer);
                drop(Box::from_raw(state));
                return Err(UsbTransferError::Other(
                    "USB isochronous packet is too large".into(),
                ));
            };
            (*transfer).iso_packet_desc.as_mut_ptr().add(index).write(
                libusb1_sys::libusb_iso_packet_descriptor {
                    length: packet_length,
                    actual_length: 0,
                    status: LIBUSB_TRANSFER_COMPLETED,
                },
            );
        }
    }

    // SAFETY: all transfer fields and descriptors are initialized above.
    let submit_result = unsafe { libusb1_sys::libusb_submit_transfer(transfer) };
    if submit_result != LIBUSB_SUCCESS {
        // SAFETY: libusb rejected the transfer, so no callback can access it.
        unsafe {
            libusb1_sys::libusb_free_transfer(transfer);
            drop(Box::from_raw(state));
        }
        return Err(map_libusb_error(submit_result));
    }

    let deadline = Instant::now().checked_add(timeout);
    let mut forced_timeout = false;
    let mut event_error = None;
    while !unsafe { (*state).done.load(Ordering::Acquire) } {
        let wait = match deadline {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    forced_timeout = true;
                    break;
                }
                remaining.min(Duration::from_millis(10))
            }
            None => Duration::from_millis(10),
        };
        match handle.context().handle_events(Some(wait)) {
            Ok(()) | Err(rusb::Error::Interrupted) => {}
            Err(error) => {
                event_error = Some(map_rusb_transfer_error(error));
                break;
            }
        }
    }

    if forced_timeout || event_error.is_some() {
        // SAFETY: the transfer was successfully submitted and has not yet
        // published completion.
        let cancel_result = unsafe { libusb1_sys::libusb_cancel_transfer(transfer) };
        if !matches!(cancel_result, LIBUSB_SUCCESS | LIBUSB_ERROR_NOT_FOUND)
            || !drain_isochronous_cancellation(handle, state)
        {
            // The transfer and its boxed state intentionally remain allocated.
            // This preserves the callback buffer and DeviceHandle lifetime when
            // a broken libusb event loop cannot acknowledge cancellation.
            return Err(event_error.unwrap_or(UsbTransferError::Timeout));
        }
    }

    // SAFETY: completion was observed with Acquire ordering, so libusb and the
    // callback no longer access the transfer state. Descriptor data is valid
    // until `libusb_free_transfer` below.
    unsafe {
        let state = Box::from_raw(state);
        let overall_status = (*transfer).status;
        let mut packets = Vec::with_capacity(packet_lengths.len());
        let mut transferred = 0usize;
        let mut offset = 0usize;
        let mut packet_error = None;
        for index in 0..packet_lengths.len() {
            let descriptor = &*(*transfer).iso_packet_desc.as_ptr().add(index);
            if descriptor.status != LIBUSB_TRANSFER_COMPLETED {
                packet_error = Some(map_libusb_transfer_status(descriptor.status));
                break;
            }
            let actual_length = descriptor.actual_length as usize;
            let requested_length = descriptor.length as usize;
            if actual_length > requested_length
                || offset
                    .checked_add(actual_length)
                    .is_none_or(|end| end > state.buffer.len())
            {
                packet_error = Some(UsbTransferError::Other(
                    "USB isochronous transfer overflow".into(),
                ));
                break;
            }
            packets.push(state.buffer[offset..offset + actual_length].to_vec());
            transferred += actual_length;
            offset += requested_length;
        }
        libusb1_sys::libusb_free_transfer(transfer);

        if forced_timeout {
            Err(UsbTransferError::Timeout)
        } else if let Some(error) = event_error {
            Err(error)
        } else if overall_status != LIBUSB_TRANSFER_COMPLETED {
            Err(map_libusb_transfer_status(overall_status))
        } else if let Some(error) = packet_error {
            Err(error)
        } else {
            Ok(IsoTransferResult {
                packets,
                transferred,
            })
        }
    }
}

fn drain_isochronous_cancellation(
    handle: &Arc<DeviceHandle<Context>>,
    state: *mut PendingIsoTransfer,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !unsafe { (*state).done.load(Ordering::Acquire) } {
        if Instant::now() >= deadline {
            return false;
        }
        match handle
            .context()
            .handle_events(Some(Duration::from_millis(10)))
        {
            Ok(()) | Err(rusb::Error::Interrupted) => {}
            Err(_) => return false,
        }
    }
    true
}

fn map_libusb_error(error: i32) -> UsbTransferError {
    match error {
        LIBUSB_ERROR_TIMEOUT => UsbTransferError::Timeout,
        LIBUSB_ERROR_NO_DEVICE => UsbTransferError::Disconnected,
        LIBUSB_ERROR_INTERRUPTED => UsbTransferError::Other("USB transfer interrupted".into()),
        error => UsbTransferError::Other(format!("libusb error {error}")),
    }
}

fn map_libusb_transfer_status(status: i32) -> UsbTransferError {
    match status {
        LIBUSB_TRANSFER_TIMED_OUT | LIBUSB_TRANSFER_CANCELLED => UsbTransferError::Timeout,
        LIBUSB_TRANSFER_NO_DEVICE => UsbTransferError::Disconnected,
        status => UsbTransferError::Other(format!("USB transfer status {status}")),
    }
}

pub type SystemUsbTransport = UsbTransport<RusbUsbIo>;

impl SystemUsbTransport {
    pub fn open(spec: &str) -> Result<Self> {
        let spec = UsbSpec::parse(spec)?;
        let context = Context::new()?;
        let devices = context.devices()?;
        let device = select_device(&devices, &spec.selector)?;
        let descriptor = device.device_descriptor()?;
        let interfaces = interface_infos(&device)?;
        let layout = select_interface_layout(&interfaces, spec.forced).ok_or_else(|| {
            Error::InvalidSpec("USB device has no compatible Bluetooth HCI interface".into())
        })?;
        let sco_layout = spec.sco_alternate.and_then(|alternate| {
            select_sco_layout(&interfaces, layout.configuration, spec.forced, alternate)
        });
        let handle = device.open()?;
        match handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(error) => return Err(error.into()),
        }
        if handle.active_configuration().ok() != Some(layout.configuration) {
            handle.set_active_configuration(layout.configuration)?;
        }
        handle.claim_interface(layout.interface)?;
        if layout.alternate != 0 {
            handle.set_alternate_setting(layout.interface, layout.alternate)?;
        }
        if let Some(sco_layout) = sco_layout {
            handle.claim_interface(sco_layout.interface)?;
            if sco_layout.alternate != 0 {
                handle.set_alternate_setting(sco_layout.interface, sco_layout.alternate)?;
            }
        }
        Ok(Self::from_backend_with_sco(
            RusbUsbIo {
                handle: Arc::new(handle),
                iso_transfer_lock: Arc::new(Mutex::new(())),
            },
            layout,
            sco_layout,
            descriptor.vendor_id(),
            descriptor.product_id(),
            device.bus_number(),
            device.address(),
        ))
    }

    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self::from_backend_with_sco(
            self.backend.clone(),
            self.layout,
            self.sco_layout,
            self.vendor_id,
            self.product_id,
            self.bus,
            self.address,
        );
        Ok((source, self))
    }
}

fn select_device<T: UsbContext>(
    devices: &rusb::DeviceList<T>,
    selector: &UsbSelector,
) -> Result<Device<T>> {
    match selector {
        UsbSelector::Index(index) => devices
            .iter()
            .filter(|device| device_is_bluetooth_hci(device).unwrap_or(false))
            .nth(*index)
            .ok_or_else(|| Error::InvalidSpec("USB device not found".into())),
        UsbSelector::Path { bus, ports } => devices
            .iter()
            .find(|device| {
                device.bus_number() == *bus
                    && device.port_numbers().ok().as_deref() == Some(ports.as_slice())
            })
            .ok_or_else(|| Error::InvalidSpec("USB device not found".into())),
        UsbSelector::VidPid {
            vendor_id,
            product_id,
            serial_number,
            occurrence,
        } => {
            let mut left = *occurrence;
            for device in devices.iter() {
                let Ok(descriptor) = device.device_descriptor() else {
                    continue;
                };
                if descriptor.vendor_id() != *vendor_id || descriptor.product_id() != *product_id {
                    continue;
                }
                if let Some(expected) = serial_number {
                    let Ok(handle) = device.open() else {
                        continue;
                    };
                    if handle
                        .read_serial_number_string_ascii(&descriptor)
                        .ok()
                        .as_deref()
                        != Some(expected.as_str())
                    {
                        continue;
                    }
                }
                if left == 0 {
                    return Ok(device);
                }
                left -= 1;
            }
            Err(Error::InvalidSpec("USB device not found".into()))
        }
    }
}

fn device_is_bluetooth_hci<T: UsbContext>(device: &Device<T>) -> Result<bool> {
    let descriptor = device.device_descriptor()?;
    let class = (
        descriptor.class_code(),
        descriptor.sub_class_code(),
        descriptor.protocol_code(),
    );
    if class
        == (
            USB_DEVICE_CLASS_WIRELESS_CONTROLLER,
            USB_DEVICE_SUBCLASS_RF_CONTROLLER,
            USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER,
        )
    {
        return Ok(true);
    }
    if descriptor.class_code() != USB_DEVICE_CLASS_DEVICE {
        return Ok(false);
    }
    Ok(interface_infos(device)?.iter().any(|interface| {
        (interface.class, interface.subclass, interface.protocol)
            == (
                USB_DEVICE_CLASS_WIRELESS_CONTROLLER,
                USB_DEVICE_SUBCLASS_RF_CONTROLLER,
                USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER,
            )
    }))
}

fn interface_infos<T: UsbContext>(device: &Device<T>) -> Result<Vec<UsbInterfaceInfo>> {
    let descriptor = device.device_descriptor()?;
    let mut infos = Vec::new();
    for config_index in 0..descriptor.num_configurations() {
        let config = device.config_descriptor(config_index)?;
        for interface in config.interfaces() {
            for setting in interface.descriptors() {
                infos.push(UsbInterfaceInfo {
                    configuration: config.number(),
                    interface: setting.interface_number(),
                    alternate: setting.setting_number(),
                    class: setting.class_code(),
                    subclass: setting.sub_class_code(),
                    protocol: setting.protocol_code(),
                    endpoints: setting
                        .endpoint_descriptors()
                        .map(|endpoint| UsbEndpointInfo {
                            address: endpoint.address(),
                            direction: endpoint.direction(),
                            transfer_type: endpoint.transfer_type(),
                            max_packet_size: endpoint.max_packet_size(),
                        })
                        .collect(),
                });
            }
        }
    }
    Ok(infos)
}

#[cfg(test)]
mod tests {
    use super::isochronous_packet_lengths;

    #[test]
    fn isochronous_output_splits_at_endpoint_boundaries() {
        assert_eq!(isochronous_packet_lengths(100, 48).unwrap(), [48, 48, 4]);
        assert_eq!(isochronous_packet_lengths(96, 48).unwrap(), [48, 48]);
        assert_eq!(
            isochronous_packet_lengths(0, 48).unwrap(),
            Vec::<usize>::new()
        );
        assert!(isochronous_packet_lengths(1, 0).is_err());
    }
}
