use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::HciPacket;
use bumble_transport::{open_split_transport, PacketSink, SplitOpenedTransport};
use std::process::ExitCode;
use std::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    transports: [String; 2],
}

fn usage() -> &'static str {
    "usage: bumble-controllers <hci-transport-1> <hci-transport-2>"
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let first = arguments.next().ok_or_else(|| usage().to_string())?;
    if matches!(first.as_str(), "-h" | "--help") {
        return Err(usage().into());
    }
    let second = arguments.next().ok_or_else(|| usage().to_string())?;
    if arguments.next().is_some() {
        return Err("exactly two HCI transports are required".into());
    }
    Ok(Args {
        transports: [first, second],
    })
}

fn drive_link(link: &mut LocalLink) {
    link.propagate_advertising();
    link.establish_connections();
    link.pump_ll();
    link.pump_periodic_sync_transfers();
    link.pump_classic();
}

fn flush_host_packets(
    link: &mut LocalLink,
    sinks: &mut [Box<dyn PacketSink + Send>],
) -> Result<(), String> {
    for (controller_id, sink) in sinks.iter_mut().enumerate() {
        let packets = link.drain_host_events(controller_id);
        for packet in &packets {
            sink.write_packet(packet)
                .map_err(|error| error.to_string())?;
        }
        if !packets.is_empty() {
            sink.flush().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn handle_host_packet(
    link: &mut LocalLink,
    sinks: &mut [Box<dyn PacketSink + Send>],
    controller_id: usize,
    packet: HciPacket,
) -> Result<(), String> {
    match packet {
        HciPacket::Command(command) => link.handle_command(controller_id, command),
        HciPacket::AclData(packet) => {
            let _ = link.send_acl_packet(controller_id, packet);
        }
        HciPacket::SyncData(packet) => {
            let _ = link.send_synchronous_data(
                controller_id,
                packet.connection_handle,
                packet.packet_status,
                &packet.data,
            );
        }
        HciPacket::IsoData(packet) => {
            let _ = link.send_iso_packet(controller_id, packet);
        }
        HciPacket::Event(_) | HciPacket::Custom(_) => {}
    }
    drive_link(link);
    flush_host_packets(link, sinks)
}

enum HostInput {
    Packet(usize, Box<HciPacket>),
    End,
    Error(String),
}

fn run_transports(transports: Vec<SplitOpenedTransport>) -> Result<(), String> {
    let mut link = LocalLink::new();
    let mut sinks = Vec::with_capacity(transports.len());
    let (input_sender, input_receiver) = mpsc::channel();

    for (controller_id, transport) in transports.into_iter().enumerate() {
        link.add_controller(Controller::new(
            &format!("C{controller_id}"),
            controller_address(controller_id)?,
        ));
        sinks.push(transport.sink);
        let mut source = transport.source;
        let input_sender = input_sender.clone();
        std::thread::spawn(move || loop {
            match source.read_packet() {
                Ok(Some(packet)) => {
                    if input_sender
                        .send(HostInput::Packet(controller_id, Box::new(packet)))
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = input_sender.send(HostInput::End);
                    return;
                }
                Err(error) => {
                    let _ = input_sender.send(HostInput::Error(error.to_string()));
                    return;
                }
            }
        });
    }
    drop(input_sender);

    loop {
        match input_receiver
            .recv()
            .map_err(|_| "controller host workers terminated unexpectedly".to_string())?
        {
            HostInput::Packet(controller_id, packet) => {
                handle_host_packet(&mut link, &mut sinks, controller_id, *packet)?;
            }
            HostInput::End => return Ok(()),
            HostInput::Error(error) => return Err(error),
        }
    }
}

fn controller_address(controller_id: usize) -> Result<Address, String> {
    let suffix = u8::try_from(controller_id)
        .ok()
        .and_then(|id| 0xF0u8.checked_add(id))
        .ok_or_else(|| format!("controller index {controller_id} exceeds the address range"))?;
    Ok(Address::from_bytes([suffix; 6], AddressType::PUBLIC_DEVICE))
}

fn run(args: Args) -> Result<(), String> {
    let mut transports = Vec::with_capacity(args.transports.len());
    for transport in args.transports {
        println!(">>> opening {transport}");
        transports.push(open_split_transport(&transport).map_err(|error| error.to_string())?);
        println!(">>> connected");
    }
    run_transports(transports)
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumble_hci::{AclDataPacket, Command, Event, LeMetaEvent};
    use bumble_transport::Result as TransportResult;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<HciPacket>>>);

    impl PacketSink for RecordingSink {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            self.0.lock().unwrap().push(packet.clone());
            Ok(())
        }
    }

    fn address(value: &str) -> Address {
        Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
    }

    fn connection_handle(packets: &[HciPacket]) -> u16 {
        packets
            .iter()
            .find_map(|packet| match packet {
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                    connection_handle,
                    ..
                })) => Some(*connection_handle),
                _ => None,
            })
            .expect("connection complete")
    }

    #[test]
    fn parses_exactly_two_transport_names() {
        assert_eq!(
            parse_args(["controllers", "pty:host-1", "pty:host-2"].map(str::to_string)),
            Ok(Args {
                transports: ["pty:host-1".into(), "pty:host-2".into()],
            })
        );
        assert!(parse_args(["controllers", "one"].map(str::to_string)).is_err());
        assert!(parse_args(["controllers", "one", "two", "three"].map(str::to_string)).is_err());
    }

    #[test]
    fn software_controllers_have_distinct_bench_compatible_public_addresses() {
        assert_eq!(
            controller_address(0).unwrap().to_string(false),
            "F0:F0:F0:F0:F0:F0"
        );
        assert_eq!(
            controller_address(1).unwrap().to_string(false),
            "F1:F1:F1:F1:F1:F1"
        );
        assert!(controller_address(16).is_err());
    }

    #[test]
    fn routes_commands_connections_and_acl_between_external_hosts() {
        let mut link = LocalLink::new();
        let public = Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE);
        link.add_controller(Controller::new("C0", public.clone()));
        link.add_controller(Controller::new("C1", public));
        let central_sink = RecordingSink::default();
        let peripheral_sink = RecordingSink::default();
        let mut sinks: Vec<Box<dyn PacketSink + Send>> = vec![
            Box::new(central_sink.clone()),
            Box::new(peripheral_sink.clone()),
        ];
        let central_address = address("C4:F2:17:1A:1D:AA");
        let peripheral_address = address("C4:F2:17:1A:1D:BB");

        for (controller_id, command) in [
            (
                1,
                Command::LeSetRandomAddress {
                    random_address: peripheral_address.clone(),
                },
            ),
            (
                1,
                Command::LeSetAdvertisingEnable {
                    advertising_enable: 1,
                },
            ),
            (
                0,
                Command::LeSetRandomAddress {
                    random_address: central_address,
                },
            ),
            (
                0,
                Command::LeCreateConnection {
                    le_scan_interval: 16,
                    le_scan_window: 16,
                    initiator_filter_policy: 0,
                    peer_address_type: 1,
                    peer_address: peripheral_address,
                    own_address_type: 1,
                    connection_interval_min: 24,
                    connection_interval_max: 40,
                    max_latency: 0,
                    supervision_timeout: 42,
                    min_ce_length: 0,
                    max_ce_length: 0,
                },
            ),
        ] {
            handle_host_packet(
                &mut link,
                &mut sinks,
                controller_id,
                HciPacket::Command(command),
            )
            .unwrap();
        }

        let central_handle = connection_handle(&central_sink.0.lock().unwrap());
        let peripheral_handle = connection_handle(&peripheral_sink.0.lock().unwrap());
        central_sink.0.lock().unwrap().clear();
        peripheral_sink.0.lock().unwrap().clear();
        let payload = vec![1, 2, 3, 4];
        handle_host_packet(
            &mut link,
            &mut sinks,
            0,
            HciPacket::AclData(AclDataPacket {
                connection_handle: central_handle,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: payload.len() as u16,
                data: payload.clone(),
            }),
        )
        .unwrap();

        assert!(central_sink.0.lock().unwrap().iter().any(|packet| matches!(
            packet,
            HciPacket::Event(Event::NumberOfCompletedPackets { .. })
        )));
        assert!(peripheral_sink
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|packet| matches!(
                packet,
                HciPacket::AclData(packet)
                    if packet.connection_handle == peripheral_handle && packet.data == payload
            )));
    }
}
