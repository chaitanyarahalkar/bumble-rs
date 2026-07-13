use crate::{Error, PacketFramer, PacketSink, PacketSource, Result, MAX_HCI_PACKET_SIZE};
use bumble_hci::{
    HciPacket, HCI_ACL_DATA_PACKET, HCI_COMMAND_PACKET, HCI_EVENT_PACKET, HCI_ISO_DATA_PACKET,
    HCI_SYNCHRONOUS_DATA_PACKET,
};
use core::fmt;
use rusb::{Device, DeviceHandle, Direction, GlobalContext, Recipient, RequestType, TransferType};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

const USB_DEVICE_CLASS_DEVICE: u8 = 0x00;
const USB_DEVICE_CLASS_WIRELESS_CONTROLLER: u8 = 0xe0;
const USB_DEVICE_SUBCLASS_RF_CONTROLLER: u8 = 0x01;
const USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER: u8 = 0x01;
const READ_TIMEOUT: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

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
}

pub struct UsbTransport<B> {
    backend: B,
    layout: UsbInterfaceLayout,
    framer: PacketFramer,
    pending: VecDeque<HciPacket>,
    poll_events_first: bool,
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
        Self {
            backend,
            layout,
            framer: PacketFramer::new(),
            pending: VecDeque::new(),
            poll_events_first: true,
            vendor_id,
            product_id,
            bus,
            address,
        }
    }

    pub fn layout(&self) -> UsbInterfaceLayout {
        self.layout
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
}

impl<B: UsbIo> PacketSource for UsbTransport<B> {
    fn read_packet(&mut self) -> Result<Option<HciPacket>> {
        if let Some(packet) = self.pending.pop_front() {
            return Ok(Some(packet));
        }
        loop {
            let first = self.poll_events_first;
            self.poll_events_first = !self.poll_events_first;
            self.poll_endpoint(first)?;
            if let Some(packet) = self.pending.pop_front() {
                return Ok(Some(packet));
            }
            self.poll_endpoint(!first)?;
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
                return Err(Error::Unsupported(
                    "USB SCO isochronous output requires an async libusb transfer backend".into(),
                ));
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
    handle: Arc<DeviceHandle<GlobalContext>>,
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
}

fn map_rusb_transfer_error(error: rusb::Error) -> UsbTransferError {
    match error {
        rusb::Error::Timeout => UsbTransferError::Timeout,
        rusb::Error::NoDevice => UsbTransferError::Disconnected,
        error => UsbTransferError::Other(error.to_string()),
    }
}

pub type SystemUsbTransport = UsbTransport<RusbUsbIo>;

impl SystemUsbTransport {
    pub fn open(spec: &str) -> Result<Self> {
        let spec = UsbSpec::parse(spec)?;
        if spec.sco_alternate.is_some() {
            return Err(Error::Unsupported(
                "USB SCO isochronous transfers are not exposed by rusb's synchronous API".into(),
            ));
        }
        let devices = rusb::devices()?;
        let device = select_device(&devices, &spec.selector)?;
        let descriptor = device.device_descriptor()?;
        let interfaces = interface_infos(&device)?;
        let layout = select_interface_layout(&interfaces, spec.forced).ok_or_else(|| {
            Error::InvalidSpec("USB device has no compatible Bluetooth HCI interface".into())
        })?;
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
        Ok(Self::from_backend(
            RusbUsbIo {
                handle: Arc::new(handle),
            },
            layout,
            descriptor.vendor_id(),
            descriptor.product_id(),
            device.bus_number(),
            device.address(),
        ))
    }

    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self::from_backend(
            self.backend.clone(),
            self.layout,
            self.vendor_id,
            self.product_id,
            self.bus,
            self.address,
        );
        Ok((source, self))
    }
}

fn select_device(
    devices: &rusb::DeviceList<GlobalContext>,
    selector: &UsbSelector,
) -> Result<Device<GlobalContext>> {
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

fn device_is_bluetooth_hci(device: &Device<GlobalContext>) -> Result<bool> {
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

fn interface_infos(device: &Device<GlobalContext>) -> Result<Vec<UsbInterfaceInfo>> {
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
