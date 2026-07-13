#![cfg(unix)]

use bumble_hci::HciPacket;
use bumble_transport::{PacketSink, PacketSource, VhciTransport, HCI_BREDR, HCI_VENDOR_PACKET};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

#[test]
fn vhci_bootstrap_filters_vendor_response_and_carries_h4() {
    let (transport_stream, mut kernel) = UnixStream::pair().unwrap();
    let command = HciPacket::from_bytes(&[0x01, 0x03, 0x0c, 0x00]).unwrap();
    let event = HciPacket::from_bytes(&[0x04, 0x0f, 0x04, 0x00, 0x01, 0x03, 0x0c]).unwrap();
    let kernel_command = command.clone();
    let kernel_event = event.clone();

    let kernel_thread = std::thread::spawn(move || {
        let mut config = [0u8; 2];
        kernel.read_exact(&mut config).unwrap();
        assert_eq!(config, [HCI_VENDOR_PACKET, HCI_BREDR]);
        kernel
            .write_all(&[HCI_VENDOR_PACKET, HCI_BREDR, 0x12, 0x34])
            .unwrap();
        kernel.write_all(&kernel_event.to_bytes()).unwrap();

        let mut bytes = vec![0u8; kernel_command.to_bytes().len()];
        kernel.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, kernel_command.to_bytes());
    });

    let mut transport = VhciTransport::from_io(transport_stream, HCI_BREDR).unwrap();
    assert_eq!(transport.hci_index(), 0x1234);
    assert_eq!(transport.controller_type(), HCI_BREDR);
    assert_eq!(transport.read_packet().unwrap(), Some(event));
    transport.write_packet(&command).unwrap();
    transport.flush().unwrap();
    kernel_thread.join().unwrap();
}

#[test]
fn vhci_rejects_non_vendor_bootstrap_response() {
    let (transport_stream, mut kernel) = UnixStream::pair().unwrap();
    let kernel_thread = std::thread::spawn(move || {
        let mut config = [0u8; 2];
        kernel.read_exact(&mut config).unwrap();
        kernel.write_all(&[0x04, 0x00, 0x00, 0x00]).unwrap();
    });
    assert!(VhciTransport::from_io(transport_stream, HCI_BREDR).is_err());
    kernel_thread.join().unwrap();
}
