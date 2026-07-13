use bumble_hci::HciPacket;
use bumble_transport::{
    select_interface_layout, PacketSink, PacketSource, UsbEndpointInfo, UsbInterfaceInfo,
    UsbInterfaceLayout, UsbIo, UsbSelector, UsbSpec, UsbTransferError, UsbTransport,
};
use rusb::{Direction, TransferType};
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Write {
    Control {
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: Vec<u8>,
    },
    Bulk {
        endpoint: u8,
        data: Vec<u8>,
    },
}

#[derive(Default)]
struct MockUsbIo {
    events: VecDeque<core::result::Result<Vec<u8>, UsbTransferError>>,
    acl: VecDeque<core::result::Result<Vec<u8>, UsbTransferError>>,
    writes: Vec<Write>,
}

impl MockUsbIo {
    fn read(
        queue: &mut VecDeque<core::result::Result<Vec<u8>, UsbTransferError>>,
        buffer: &mut [u8],
    ) -> core::result::Result<usize, UsbTransferError> {
        match queue.pop_front().unwrap_or(Err(UsbTransferError::Timeout)) {
            Ok(data) => {
                buffer[..data.len()].copy_from_slice(&data);
                Ok(data.len())
            }
            Err(error) => Err(error),
        }
    }
}

impl UsbIo for MockUsbIo {
    fn read_interrupt(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        assert_eq!(endpoint, 0x81);
        Self::read(&mut self.events, buffer)
    }

    fn read_bulk(
        &mut self,
        endpoint: u8,
        buffer: &mut [u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        assert_eq!(endpoint, 0x82);
        Self::read(&mut self.acl, buffer)
    }

    fn write_control(
        &mut self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        buffer: &[u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.writes.push(Write::Control {
            request_type,
            request,
            value,
            index,
            data: buffer.to_vec(),
        });
        Ok(buffer.len())
    }

    fn write_bulk(
        &mut self,
        endpoint: u8,
        buffer: &[u8],
        _timeout: Duration,
    ) -> core::result::Result<usize, UsbTransferError> {
        self.writes.push(Write::Bulk {
            endpoint,
            data: buffer.to_vec(),
        });
        Ok(buffer.len())
    }
}

fn layout() -> UsbInterfaceLayout {
    UsbInterfaceLayout {
        configuration: 1,
        interface: 0,
        alternate: 0,
        interrupt_in: 0x81,
        bulk_in: 0x82,
        bulk_out: 0x02,
    }
}

fn endpoint(address: u8, direction: Direction, transfer_type: TransferType) -> UsbEndpointInfo {
    UsbEndpointInfo {
        address,
        direction,
        transfer_type,
        max_packet_size: 64,
    }
}

#[test]
fn usb_spec_covers_upstream_selector_forms() {
    assert_eq!(
        UsbSpec::parse("0").unwrap(),
        UsbSpec {
            selector: UsbSelector::Index(0),
            forced: false,
            sco_alternate: None,
        }
    );
    assert_eq!(
        UsbSpec::parse("04b4:f901#2").unwrap().selector,
        UsbSelector::VidPid {
            vendor_id: 0x04b4,
            product_id: 0xf901,
            serial_number: None,
            occurrence: 2,
        }
    );
    assert_eq!(
        UsbSpec::parse("04b4:f901/00E04C239987").unwrap().selector,
        UsbSelector::VidPid {
            vendor_id: 0x04b4,
            product_id: 0xf901,
            serial_number: Some("00E04C239987".into()),
            occurrence: 0,
        }
    );
    assert_eq!(
        UsbSpec::parse("3-1.4.2").unwrap().selector,
        UsbSelector::Path {
            bus: 3,
            ports: vec![1, 4, 2],
        }
    );
    let forced_sco = UsbSpec::parse("0+sco=5!").unwrap();
    assert!(forced_sco.forced);
    assert_eq!(forced_sco.sco_alternate, Some(5));
    assert!(UsbSpec::parse("not-an-index").is_err());
}

#[test]
fn interface_selection_requires_one_complete_bluetooth_setting() {
    let wrong = UsbInterfaceInfo {
        configuration: 1,
        interface: 0,
        alternate: 0,
        class: 0xff,
        subclass: 0,
        protocol: 0,
        endpoints: vec![
            endpoint(0x81, Direction::In, TransferType::Interrupt),
            endpoint(0x82, Direction::In, TransferType::Bulk),
            endpoint(0x02, Direction::Out, TransferType::Bulk),
        ],
    };
    let mut bluetooth = wrong.clone();
    bluetooth.interface = 2;
    bluetooth.alternate = 1;
    bluetooth.class = 0xe0;
    bluetooth.subclass = 1;
    bluetooth.protocol = 1;
    assert_eq!(
        select_interface_layout(&[wrong.clone(), bluetooth], false),
        Some(UsbInterfaceLayout {
            interface: 2,
            alternate: 1,
            ..layout()
        })
    );
    assert_eq!(select_interface_layout(&[wrong], true), Some(layout()));

    let incomplete = UsbInterfaceInfo {
        endpoints: vec![endpoint(0x81, Direction::In, TransferType::Interrupt)],
        ..UsbInterfaceInfo {
            configuration: 1,
            interface: 0,
            alternate: 0,
            class: 0xe0,
            subclass: 1,
            protocol: 1,
            endpoints: Vec::new(),
        }
    };
    assert_eq!(select_interface_layout(&[incomplete], false), None);
}

#[test]
fn usb_transport_polls_event_and_acl_endpoints_without_starvation() {
    let event = HciPacket::from_bytes(&[0x04, 0x0f, 0x04, 0x00, 0x01, 0x03, 0x0c]).unwrap();
    let acl = HciPacket::from_bytes(&[0x02, 0x01, 0x20, 0x03, 0x00, 1, 2, 3]).unwrap();
    let mut backend = MockUsbIo::default();
    backend.events.push_back(Ok(event.to_bytes()[1..].to_vec()));
    backend.acl.push_back(Ok(acl.to_bytes()[1..].to_vec()));
    let mut transport = UsbTransport::from_backend(backend, layout(), 0x1234, 0xabcd, 3, 7);

    assert_eq!(transport.read_packet().unwrap(), Some(event));
    assert_eq!(transport.read_packet().unwrap(), Some(acl));
    assert_eq!(transport.vendor_id(), 0x1234);
    assert_eq!(transport.product_id(), 0xabcd);
    assert_eq!(transport.bus(), 3);
    assert_eq!(transport.address(), 7);
}

#[test]
fn usb_transport_routes_command_acl_and_iso_writes() {
    let command = HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap();
    let acl = HciPacket::from_bytes(&[0x02, 0x01, 0x20, 0x03, 0x00, 1, 2, 3]).unwrap();
    let iso = HciPacket::from_bytes(&[0x05, 0x01, 0x10, 0x02, 0x00, 4, 5]).unwrap();
    let sync = HciPacket::from_bytes(&[0x03, 0x01, 0x00, 0x02, 6, 7]).unwrap();
    let mut transport = UsbTransport::from_backend(MockUsbIo::default(), layout(), 0, 0, 0, 0);

    transport.write_packet(&command).unwrap();
    transport.write_packet(&acl).unwrap();
    transport.write_packet(&iso).unwrap();
    assert!(transport.write_packet(&sync).is_err());
    assert_eq!(
        transport.get_ref().writes,
        vec![
            Write::Control {
                request_type: 0x20,
                request: 0,
                value: 0,
                index: 0,
                data: command.to_bytes()[1..].to_vec(),
            },
            Write::Bulk {
                endpoint: 0x02,
                data: acl.to_bytes()[1..].to_vec(),
            },
            Write::Bulk {
                endpoint: 0x02,
                data: iso.to_bytes()[1..].to_vec(),
            },
        ]
    );
}

#[test]
fn usb_transport_propagates_disconnects() {
    let mut backend = MockUsbIo::default();
    backend
        .events
        .push_back(Err(UsbTransferError::Disconnected));
    let mut transport = UsbTransport::from_backend(backend, layout(), 0, 0, 0, 0);
    assert!(transport.read_packet().is_err());
}
