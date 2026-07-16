use std::sync::{Arc, Mutex};

use bumble::{Address, AddressType};
use bumble_hci::{
    AclDataPacket, AdvertisingReport, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent,
    ReturnParameters, HCI_RESET_COMMAND,
};
use bumble_host::{
    ChannelSoundingSubeventResult, ChannelSoundingSubeventResultContinue, ClassicPairingEvent,
    Device, DeviceConnectionTransport, DeviceEvent, HostTransport, LeConnectionControlEvent,
    LeDataLength, PeerLookupTransport, RemoteNameError,
};

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[derive(Default)]
struct ScriptedTransport {
    events: Vec<HciPacket>,
}

#[test]
fn channel_sounding_subevents_and_vendor_events_are_retained_and_published() {
    let handle = 0x0BAD;
    let first = ChannelSoundingSubeventResult {
        connection_handle: handle,
        config_id: 2,
        start_acl_conn_event_counter: 0x1234,
        procedure_counter: 0x5678,
        frequency_compensation: 0x9ABC,
        reference_power_level: -12,
        procedure_done_status: 1,
        subevent_done_status: 2,
        abort_reason: 3,
        num_antenna_paths: 4,
        step_mode: vec![1, 2],
        step_channel: vec![7, 8],
        step_data: vec![vec![0xAA, 0xBB], vec![0xCC]],
    };
    let continuation = ChannelSoundingSubeventResultContinue {
        connection_handle: handle,
        config_id: 2,
        procedure_done_status: 4,
        subevent_done_status: 5,
        abort_reason: 6,
        num_antenna_paths: 7,
        step_mode: vec![3],
        step_channel: vec![9],
        step_data: vec![vec![0xDD, 0xEE]],
    };
    let vendor_data = vec![0x01, 0xFE, 0x80, 0x00];
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSubeventResult {
                connection_handle: first.connection_handle,
                config_id: first.config_id,
                start_acl_conn_event_counter: first.start_acl_conn_event_counter,
                procedure_counter: first.procedure_counter,
                frequency_compensation: first.frequency_compensation,
                reference_power_level: first.reference_power_level,
                procedure_done_status: first.procedure_done_status,
                subevent_done_status: first.subevent_done_status,
                abort_reason: first.abort_reason,
                num_antenna_paths: first.num_antenna_paths,
                step_mode: first.step_mode.clone(),
                step_channel: first.step_channel.clone(),
                step_data: first.step_data.clone(),
            })),
            HciPacket::Event(Event::Vendor {
                data: vendor_data.clone(),
            }),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSubeventResultContinue {
                connection_handle: continuation.connection_handle,
                config_id: continuation.config_id,
                procedure_done_status: continuation.procedure_done_status,
                subevent_done_status: continuation.subevent_done_status,
                abort_reason: continuation.abort_reason,
                num_antenna_paths: continuation.num_antenna_paths,
                step_mode: continuation.step_mode.clone(),
                step_channel: continuation.step_channel.clone(),
                step_data: continuation.step_data.clone(),
            })),
        ],
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_events = Arc::clone(&observed);
    let mut device = Device::new(0);
    device.add_event_listener(move |event| {
        listener_events.lock().unwrap().push(event.clone());
    });

    assert!(device.poll(&mut transport));
    assert_eq!(
        device.take_channel_sounding_subevent_results(),
        vec![first.clone()]
    );
    assert_eq!(device.take_vendor_events(), vec![vendor_data.clone()]);
    assert_eq!(
        device.take_channel_sounding_subevent_result_continuations(),
        vec![continuation.clone()]
    );
    let journal = device.take_device_events();
    assert_eq!(
        journal,
        vec![
            DeviceEvent::ChannelSoundingSubeventResult(first),
            DeviceEvent::VendorEvent(vendor_data),
            DeviceEvent::ChannelSoundingSubeventResultContinue(continuation),
        ]
    );
    assert_eq!(*observed.lock().unwrap(), journal);
}

#[test]
fn reset_gates_controller_packets_until_a_successful_completion() {
    let mut transport = ScriptedTransport::default();
    let mut device = Device::new(0);

    device.reset(&mut transport);
    assert!(!device.controller_ready());
    assert_eq!(device.reset_status(), None);
    transport.events.extend([
        HciPacket::Event(Event::Vendor { data: vec![1] }),
        HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: HCI_RESET_COMMAND,
            return_parameters: ReturnParameters::Status { status: 0 },
        }),
        HciPacket::Event(Event::Vendor { data: vec![2] }),
    ]);

    assert!(device.poll(&mut transport));
    assert!(device.controller_ready());
    assert_eq!(device.reset_status(), Some(0));
    assert_eq!(device.take_vendor_events(), vec![vec![2]]);
    assert_eq!(
        device.take_device_events(),
        vec![DeviceEvent::VendorEvent(vec![2])]
    );

    device.reset(&mut transport);
    transport.events.extend([
        HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: HCI_RESET_COMMAND,
            return_parameters: ReturnParameters::Status { status: 0x0C },
        }),
        HciPacket::Event(Event::Vendor { data: vec![3] }),
    ]);

    assert!(device.poll(&mut transport));
    assert!(!device.controller_ready());
    assert_eq!(device.reset_status(), Some(0x0C));
    assert!(device.take_vendor_events().is_empty());
    assert!(device.take_device_events().is_empty());
}

#[test]
fn transport_loss_flushes_connections_through_the_normal_event_path() {
    let handle = 0x0042;
    let mut transport = ScriptedTransport {
        events: vec![HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: handle,
                role: 0,
                peer_address_type: 1,
                peer_address: random_address("C0:00:00:00:00:42"),
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            },
        ))],
    };
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));
    assert!(device.is_connected_on_handle(handle));
    device.take_device_events();

    device.on_transport_lost();

    assert!(!device.is_connected_on_handle(handle));
    assert_eq!(
        device.take_device_events(),
        vec![
            DeviceEvent::Flush,
            DeviceEvent::Disconnected {
                connection_handle: handle,
                reason: 0,
            },
        ]
    );
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
        DeviceEvent::RemoteNameFailure {
            peer_address: classic_peer,
            error: RemoteNameError::HciStatus(0x02),
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
fn flush_invalidates_every_connection_after_the_flush_event() {
    let le_peer = random_address("C0:00:00:00:00:40");
    let classic_peer = random_address("C0:00:00:00:00:50");
    let mut transport = ScriptedTransport {
        events: vec![
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                status: 0,
                connection_handle: 0x0040,
                role: 0,
                peer_address_type: 1,
                peer_address: le_peer,
                connection_interval: 24,
                peripheral_latency: 0,
                supervision_timeout: 72,
                central_clock_accuracy: 0,
            })),
            HciPacket::Event(Event::ConnectionComplete {
                status: 0,
                connection_handle: 0x0050,
                bd_addr: classic_peer.clone(),
                link_type: 1,
                encryption_enabled: 0,
            }),
            HciPacket::Event(Event::SynchronousConnectionComplete {
                status: 0,
                connection_handle: 0x0051,
                bd_addr: classic_peer,
                link_type: 2,
                transmission_interval: 12,
                retransmission_window: 2,
                rx_packet_length: 60,
                tx_packet_length: 60,
                air_mode: 2,
            }),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CisEstablished {
                status: 0,
                connection_handle: 0x0060,
                cig_sync_delay: 1,
                cis_sync_delay: 2,
                transport_latency_c_to_p: 3,
                transport_latency_p_to_c: 4,
                phy_c_to_p: 1,
                phy_p_to_c: 2,
                nse: 3,
                bn_c_to_p: 4,
                bn_p_to_c: 5,
                ft_c_to_p: 6,
                ft_p_to_c: 7,
                max_pdu_c_to_p: 120,
                max_pdu_p_to_c: 121,
                iso_interval: 8,
            })),
        ],
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let listener_events = Arc::clone(&observed);
    let mut device = Device::new(0);
    assert!(device.poll(&mut transport));
    device.take_device_events();
    device.add_event_listener(move |event| {
        listener_events.lock().unwrap().push(event.clone());
    });
    let lookup = device.find_peer_by_name(&mut transport, "pending", PeerLookupTransport::Le);
    assert!(device.is_peer_lookup_pending(lookup));

    device.flush();

    assert!(!device.is_connected());
    assert!(device.le_connections().next().is_none());
    assert!(device.classic_connections().next().is_none());
    assert!(device.synchronous_connections().is_empty());
    assert!(device.cis_link(0x0060).is_none());
    assert_eq!(device.pending_peer_lookup_count(), 0);
    assert_eq!(device.acl_packets_pending(), 0);
    let expected = vec![
        DeviceEvent::Flush,
        DeviceEvent::Disconnected {
            connection_handle: 0x0040,
            reason: 0,
        },
        DeviceEvent::Disconnected {
            connection_handle: 0x0050,
            reason: 0,
        },
        DeviceEvent::Disconnected {
            connection_handle: 0x0051,
            reason: 0,
        },
        DeviceEvent::Disconnected {
            connection_handle: 0x0060,
            reason: 0,
        },
    ];
    assert_eq!(device.take_device_events(), expected);
    assert_eq!(*observed.lock().unwrap(), expected);

    device.power_off();
    assert!(!device.is_scanning());
    assert!(device.take_device_events().is_empty());
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
