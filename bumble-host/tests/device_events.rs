use std::sync::{Arc, Mutex};

use bumble::{Address, AddressType};
use bumble_hci::{
    AclDataPacket, AdvertisingReport, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent,
};
use bumble_host::{
    ClassicPairingEvent, Device, DeviceConnectionTransport, DeviceEvent, HostTransport,
    LeConnectionControlEvent, LeDataLength,
};

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[derive(Default)]
struct ScriptedTransport {
    events: Vec<HciPacket>,
}

impl HostTransport for ScriptedTransport {
    fn handle_command(&mut self, _controller_id: usize, _command: Command) {}

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
    }

    fn send_synchronous_data(
        &mut self,
        _controller_id: usize,
        _connection_handle: u16,
        _packet_status: u8,
        _data: &[u8],
    ) -> bool {
        false
    }

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        core::mem::take(&mut self.events)
    }
}

#[test]
fn listeners_observe_post_state_events_and_failed_disconnects_preserve_state() {
    let handle = 0x0040;
    let peer = random_address("C0:00:00:00:00:02");
    let report = AdvertisingReport {
        event_type: 0,
        address_type: 1,
        address: peer.clone(),
        data: vec![2, 1, 6],
        rssi: -42,
    };
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: handle,
                role: 0,
                peer_address_type: 1,
                peer_address: peer.clone(),
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            })),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport {
                reports: vec![report.clone()],
            })),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::DataLengthChange {
                connection_handle: handle,
                max_tx_octets: 200,
                max_tx_time: 1_000,
                max_rx_octets: 180,
                max_rx_time: 900,
            })),
            HciPacket::Event(Event::EncryptionChange {
                status: 0,
                connection_handle: handle,
                encryption_enabled: 1,
            }),
            HciPacket::Event(Event::DisconnectionComplete {
                status: 0x0C,
                connection_handle: handle,
                reason: 0x13,
            }),
        ],
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_events = Arc::clone(&observed);
    let mut device = Device::new(0);
    let listener_id = device.add_event_listener(move |event| {
        listener_events.lock().unwrap().push(event.clone());
    });

    assert!(device.poll(&mut transport));
    let journal = device.take_device_events();
    assert_eq!(*observed.lock().unwrap(), journal);
    assert_eq!(journal.len(), 5);
    assert!(matches!(
        &journal[0],
        DeviceEvent::LeConnectionEstablished(connection)
            if connection.connection_handle == handle && connection.peer_address == peer
    ));
    assert_eq!(journal[1], DeviceEvent::AdvertisingReport(report));
    assert_eq!(
        journal[2],
        DeviceEvent::LeConnectionControl(LeConnectionControlEvent::DataLengthChange {
            connection_handle: handle,
            data_length: LeDataLength {
                max_tx_octets: 200,
                max_tx_time: 1_000,
                max_rx_octets: 180,
                max_rx_time: 900,
            },
        })
    );
    assert_eq!(
        journal[3],
        DeviceEvent::EncryptionChange {
            status: 0,
            connection_handle: handle,
            encryption_enabled: 1,
            encryption_key_size: 0,
        }
    );
    assert_eq!(
        journal[4],
        DeviceEvent::DisconnectionFailed {
            connection_handle: handle,
            status: 0x0C,
        }
    );

    let connection = device.le_connection(handle).unwrap();
    assert_eq!(
        connection.data_length,
        Some(LeDataLength {
            max_tx_octets: 200,
            max_tx_time: 1_000,
            max_rx_octets: 180,
            max_rx_time: 900,
        })
    );
    assert!(device.is_encrypted_on_handle(handle));

    transport
        .events
        .push(HciPacket::Event(Event::DisconnectionComplete {
            status: 0,
            connection_handle: handle,
            reason: 0x13,
        }));
    assert!(device.poll(&mut transport));
    assert!(!device.is_connected_on_handle(handle));
    assert_eq!(
        device.take_device_events(),
        vec![DeviceEvent::Disconnected {
            connection_handle: handle,
            reason: 0x13,
        }]
    );
    assert_eq!(observed.lock().unwrap().len(), 6);

    assert!(device.remove_event_listener(listener_id));
    assert!(!device.remove_event_listener(listener_id));
    transport
        .events
        .push(HciPacket::Event(Event::InquiryComplete { status: 0 }));
    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_device_events(),
        vec![DeviceEvent::InquiryComplete { status: 0 }]
    );
    assert_eq!(observed.lock().unwrap().len(), 6);
}

#[test]
fn failed_disconnection_clears_pending_state_without_removing_connection() {
    let handle = 0x0041;
    let peer = random_address("C0:00:00:00:00:03");
    let mut transport = ScriptedTransport {
        events: vec![HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: handle,
                role: 0,
                peer_address_type: 1,
                peer_address: peer,
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            },
        ))],
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));

    assert!(device.disconnect_handle(&mut transport, handle, 0x13));
    assert!(device.is_disconnecting());
    assert!(device.is_disconnecting_on_handle(handle));
    transport
        .events
        .push(HciPacket::Event(Event::DisconnectionComplete {
            status: 0x0C,
            connection_handle: handle,
            reason: 0x13,
        }));
    assert!(device.poll(&mut transport));

    assert!(!device.is_disconnecting());
    assert!(!device.is_disconnecting_on_handle(handle));
    assert!(device.is_connected_on_handle(handle));
    assert_eq!(
        device.take_device_events().last(),
        Some(&DeviceEvent::DisconnectionFailed {
            connection_handle: handle,
            status: 0x0C,
        })
    );
}

#[test]
fn connection_failures_discovery_and_pairing_are_typed_device_events() {
    let le_peer = random_address("C0:00:00:00:00:11");
    let classic_peer = random_address("C0:00:00:00:00:22");
    let sync_peer = random_address("C0:00:00:00:00:33");
    let mut remote_name = [0u8; 248];
    remote_name[..5].copy_from_slice(b"radio");
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                status: 0x3E,
                connection_handle: 0,
                role: 0,
                peer_address_type: 1,
                peer_address: le_peer.clone(),
                connection_interval: 0,
                peripheral_latency: 0,
                supervision_timeout: 0,
                central_clock_accuracy: 0,
            })),
            HciPacket::Event(Event::ConnectionComplete {
                status: 0x04,
                connection_handle: 0,
                bd_addr: classic_peer.clone(),
                link_type: 1,
                encryption_enabled: 0,
            }),
            HciPacket::Event(Event::SynchronousConnectionComplete {
                status: 0x0D,
                connection_handle: 0,
                bd_addr: sync_peer.clone(),
                link_type: 2,
                transmission_interval: 0,
                retransmission_window: 0,
                rx_packet_length: 0,
                tx_packet_length: 0,
                air_mode: 0,
            }),
            HciPacket::Event(Event::ConnectionRequest {
                bd_addr: classic_peer.clone(),
                class_of_device: 0x200404,
                link_type: 1,
            }),
            HciPacket::Event(Event::PinCodeRequest {
                bd_addr: classic_peer.clone(),
            }),
            HciPacket::Event(Event::InquiryResultWithRssi {
                bd_addr: vec![classic_peer.clone()],
                page_scan_repetition_mode: vec![1],
                reserved: vec![0],
                class_of_device: vec![0x200404],
                clock_offset: vec![0x1234],
                rssi: vec![-31],
            }),
            HciPacket::Event(Event::RemoteNameRequestComplete {
                status: 0x02,
                bd_addr: classic_peer.clone(),
                remote_name,
            }),
        ],
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));

    let events = device.take_device_events();
    assert_eq!(events.len(), 7);
    assert_eq!(
        events[0],
        DeviceEvent::ConnectionFailed {
            transport: DeviceConnectionTransport::Le,
            peer_address: le_peer,
            status: 0x3E,
        }
    );
    assert_eq!(
        events[1],
        DeviceEvent::ConnectionFailed {
            transport: DeviceConnectionTransport::Classic,
            peer_address: classic_peer.clone(),
            status: 0x04,
        }
    );
    assert_eq!(
        events[2],
        DeviceEvent::ConnectionFailed {
            transport: DeviceConnectionTransport::Synchronous { link_type: 2 },
            peer_address: sync_peer,
            status: 0x0D,
        }
    );
    assert_eq!(
        events[3],
        DeviceEvent::ConnectionRequest {
            peer_address: classic_peer.clone(),
            class_of_device: 0x200404,
            link_type: 1,
        }
    );
    assert_eq!(
        events[4],
        DeviceEvent::ClassicPairing(ClassicPairingEvent::PinCodeRequest {
            peer_address: classic_peer.clone(),
        })
    );
    assert!(matches!(
        &events[5],
        DeviceEvent::InquiryResult(result)
            if result.peer_address == classic_peer
                && result.class_of_device == 0x200404
                && result.rssi == Some(-31)
    ));
    assert_eq!(
        events[6],
        DeviceEvent::RemoteName {
            status: 0x02,
            peer_address: classic_peer,
            name: "radio".into(),
        }
    );
    assert!(!device.is_connected());
    assert!(device.classic_connections().next().is_none());
}

#[test]
fn classic_and_synchronous_success_events_include_retained_connection_state() {
    let peer = random_address("C0:00:00:00:00:44");
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::ConnectionComplete {
                status: 0,
                connection_handle: 0x0050,
                bd_addr: peer.clone(),
                link_type: 1,
                encryption_enabled: 0,
            }),
            HciPacket::Event(Event::SynchronousConnectionComplete {
                status: 0,
                connection_handle: 0x0051,
                bd_addr: peer.clone(),
                link_type: 2,
                transmission_interval: 12,
                retransmission_window: 2,
                rx_packet_length: 60,
                tx_packet_length: 60,
                air_mode: 2,
            }),
        ],
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));

    let classic = device.classic_connection(0x0050).unwrap().clone();
    let synchronous = device.synchronous_connections()[0].clone();
    assert_eq!(classic.peer_address, peer);
    assert_eq!(synchronous.connection_handle, 0x0051);
    assert_eq!(synchronous.peer_address, peer);
    assert_eq!(synchronous.link_type, 2);
    assert_eq!(synchronous.air_mode, 2);
    assert_eq!(
        device.take_device_events(),
        vec![
            DeviceEvent::ClassicConnectionEstablished(classic),
            DeviceEvent::SynchronousConnectionEstablished(synchronous),
        ]
    );
}

#[test]
fn encryption_qos_and_remote_host_features_match_upstream_routing() {
    let handle = 0x0060;
    let peer = random_address("C0:00:00:00:00:60");
    let mut transport = ScriptedTransport {
        events: vec![HciPacket::Event(Event::ConnectionComplete {
            status: 0,
            connection_handle: handle,
            bd_addr: peer.clone(),
            link_type: 1,
            encryption_enabled: 1,
        })],
    };
    let mut device = Device::new(0);

    assert!(device.poll(&mut transport));
    let connection = device.classic_connection(handle).unwrap();
    assert_eq!(connection.encryption_enabled, 1);
    assert_eq!(connection.encryption_key_size, 0);
    assert!(device.is_classic_encrypted());
    assert!(matches!(
        device.take_device_events().as_slice(),
        [DeviceEvent::ClassicConnectionEstablished(connection)]
            if connection.encryption_enabled == 1
    ));

    let host_supported_features = [1, 2, 3, 4, 5, 6, 7, 8];
    transport.events.extend([
        HciPacket::Event(Event::EncryptionChangeV2 {
            status: 0,
            connection_handle: handle,
            encryption_enabled: 2,
            encryption_key_size: 16,
        }),
        HciPacket::Event(Event::QosSetupComplete {
            status: 0,
            connection_handle: handle,
            unused: 0,
            service_type: 2,
        }),
        HciPacket::Event(Event::RemoteHostSupportedFeaturesNotification {
            bd_addr: peer.clone(),
            host_supported_features,
        }),
        HciPacket::Event(Event::EncryptionKeyRefreshComplete {
            status: 0,
            connection_handle: handle,
        }),
    ]);
    assert!(device.poll(&mut transport));

    let connection = device.classic_connection(handle).unwrap();
    assert_eq!(connection.encryption_enabled, 2);
    assert_eq!(connection.encryption_key_size, 16);
    assert_eq!(connection.qos_service_type, Some(2));
    assert_eq!(
        connection.peer_host_supported_features,
        Some(host_supported_features)
    );
    assert_eq!(
        device.take_device_events(),
        vec![
            DeviceEvent::EncryptionChange {
                status: 0,
                connection_handle: handle,
                encryption_enabled: 2,
                encryption_key_size: 16,
            },
            DeviceEvent::QosSetup {
                connection_handle: handle,
                service_type: 2,
            },
            DeviceEvent::RemoteHostSupportedFeatures {
                peer_address: peer,
                host_supported_features,
            },
            DeviceEvent::EncryptionKeyRefresh {
                connection_handle: handle,
            },
        ]
    );

    transport.events.extend([
        HciPacket::Event(Event::EncryptionChange {
            status: 0x0C,
            connection_handle: handle,
            encryption_enabled: 0,
        }),
        HciPacket::Event(Event::QosSetupComplete {
            status: 0x0D,
            connection_handle: handle,
            unused: 0,
            service_type: 0,
        }),
        HciPacket::Event(Event::EncryptionKeyRefreshComplete {
            status: 0x06,
            connection_handle: handle,
        }),
    ]);
    assert!(device.poll(&mut transport));

    // Upstream only mutates encryption/QoS state on success.
    let connection = device.classic_connection(handle).unwrap();
    assert_eq!(connection.encryption_enabled, 2);
    assert_eq!(connection.encryption_key_size, 16);
    assert_eq!(connection.qos_service_type, Some(2));
    assert!(device.is_classic_encrypted());
    assert_eq!(
        device.take_device_events(),
        vec![
            DeviceEvent::EncryptionChange {
                status: 0x0C,
                connection_handle: handle,
                encryption_enabled: 0,
                encryption_key_size: 0,
            },
            DeviceEvent::QosSetupFailed {
                connection_handle: handle,
                status: 0x0D,
            },
            DeviceEvent::EncryptionKeyRefreshFailed {
                connection_handle: handle,
                status: 0x06,
            },
        ]
    );
}
