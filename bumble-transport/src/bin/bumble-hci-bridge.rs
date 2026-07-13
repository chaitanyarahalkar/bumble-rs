use bumble_hci::{op_code, Event, HciPacket, ReturnParameters};
use bumble_transport::{open_split_transport, PacketSink, PacketSource};
use std::collections::BTreeSet;
use std::process::ExitCode;
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Args {
    host_transport: String,
    controller_transport: String,
    command_short_circuits: BTreeSet<u16>,
}

fn usage() -> &'static str {
    "usage: bumble-hci-bridge <host-transport-spec> <controller-transport-spec> [command-short-circuit-list]"
}

fn parse_hex(value: &str, name: &str) -> Result<u16, String> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if digits.is_empty() {
        return Err(format!("empty {name}"));
    }
    u16::from_str_radix(digits, 16).map_err(|_| format!("invalid {name} {value:?}"))
}

fn parse_short_circuits(value: &str) -> Result<BTreeSet<u16>, String> {
    value
        .split(',')
        .map(|entry| {
            if let Some((ogf, ocf)) = entry.split_once(':') {
                let ogf = parse_hex(ogf, "OGF")?;
                let ocf = parse_hex(ocf, "OCF")?;
                if ogf > 0x3F {
                    return Err(format!("OGF {ogf:#x} exceeds 6 bits"));
                }
                if ocf > 0x03FF {
                    return Err(format!("OCF {ocf:#x} exceeds 10 bits"));
                }
                Ok(op_code(ogf as u8, ocf))
            } else {
                parse_hex(entry, "command opcode")
            }
        })
        .collect()
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let host_transport = arguments.next().ok_or_else(|| usage().to_string())?;
    if matches!(host_transport.as_str(), "-h" | "--help") {
        return Err(usage().into());
    }
    let controller_transport = arguments.next().ok_or_else(|| usage().to_string())?;
    let command_short_circuits = arguments
        .next()
        .map(|value| parse_short_circuits(&value))
        .transpose()?
        .unwrap_or_default();
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }
    Ok(Args {
        host_transport,
        controller_transport,
        command_short_circuits,
    })
}

type SharedSink = Arc<Mutex<Box<dyn PacketSink + Send>>>;

fn write_shared(sink: &SharedSink, packet: &HciPacket) -> Result<(), String> {
    let mut sink = sink
        .lock()
        .map_err(|_| "host transport sink lock is poisoned".to_string())?;
    sink.write_packet(packet)
        .map_err(|error| error.to_string())?;
    sink.flush().map_err(|error| error.to_string())
}

fn pump_host_to_controller(
    mut host_source: Box<dyn PacketSource + Send>,
    host_sink: SharedSink,
    mut controller_sink: Box<dyn PacketSink + Send>,
    command_short_circuits: Arc<BTreeSet<u16>>,
) -> Result<(), String> {
    while let Some(packet) = host_source
        .read_packet()
        .map_err(|error| error.to_string())?
    {
        if let HciPacket::Command(command) = &packet {
            let command_opcode = command.op_code();
            if command_short_circuits.contains(&command_opcode) {
                write_shared(
                    &host_sink,
                    &HciPacket::Event(Event::CommandComplete {
                        num_hci_command_packets: 1,
                        command_opcode,
                        return_parameters: ReturnParameters::Status { status: 0 },
                    }),
                )?;
                continue;
            }
        }
        controller_sink
            .write_packet(&packet)
            .map_err(|error| error.to_string())?;
        controller_sink.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn pump_controller_to_host(
    mut controller_source: Box<dyn PacketSource + Send>,
    host_sink: SharedSink,
) -> Result<(), String> {
    while let Some(packet) = controller_source
        .read_packet()
        .map_err(|error| error.to_string())?
    {
        write_shared(&host_sink, &packet)?;
    }
    Ok(())
}

fn run(args: Args) -> Result<(), String> {
    println!(">>> connecting host HCI transport...");
    let host = open_split_transport(&args.host_transport).map_err(|error| error.to_string())?;
    println!(">>> host connected");
    println!(">>> connecting controller HCI transport...");
    let controller =
        open_split_transport(&args.controller_transport).map_err(|error| error.to_string())?;
    println!(">>> controller connected");

    let host_sink: SharedSink = Arc::new(Mutex::new(host.sink));
    let command_short_circuits = Arc::new(args.command_short_circuits);
    let (completion_sender, completion_receiver) = mpsc::channel();

    let host_completion = completion_sender.clone();
    let host_reply_sink = Arc::clone(&host_sink);
    std::thread::spawn(move || {
        let result = pump_host_to_controller(
            host.source,
            host_reply_sink,
            controller.sink,
            command_short_circuits,
        );
        let _ = host_completion.send(result);
    });

    std::thread::spawn(move || {
        let result = pump_controller_to_host(controller.source, host_sink);
        let _ = completion_sender.send(result);
    });

    completion_receiver
        .recv()
        .map_err(|_| "HCI bridge workers terminated unexpectedly".to_string())?
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
    use bumble_hci::Command;
    use bumble_transport::Result as TransportResult;
    use std::collections::VecDeque;

    struct MockSource(VecDeque<HciPacket>);

    impl PacketSource for MockSource {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            Ok(self.0.pop_front())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<HciPacket>>>);

    impl PacketSink for RecordingSink {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            self.0.lock().unwrap().push(packet.clone());
            Ok(())
        }
    }

    fn boxed_source(packets: Vec<HciPacket>) -> Box<dyn PacketSource + Send> {
        Box::new(MockSource(packets.into()))
    }

    fn shared_sink(sink: RecordingSink) -> SharedSink {
        Arc::new(Mutex::new(Box::new(sink)))
    }

    #[test]
    fn parses_opcode_and_ogf_ocf_short_circuits() {
        assert_eq!(
            parse_args(
                [
                    "bridge",
                    "udp:host",
                    "serial:controller",
                    "0x0c03,0x3f:0x0070",
                ]
                .map(str::to_string)
            ),
            Ok(Args {
                host_transport: "udp:host".into(),
                controller_transport: "serial:controller".into(),
                command_short_circuits: BTreeSet::from([0x0C03, 0xFC70]),
            })
        );
        assert!(parse_short_circuits("0x40:1").is_err());
        assert!(parse_short_circuits("1:0x400").is_err());
        assert!(parse_args(["bridge", "only-one"].map(str::to_string)).is_err());
    }

    #[test]
    fn host_pump_short_circuits_selected_commands_and_forwards_others() {
        let host_sink = RecordingSink::default();
        let controller_sink = RecordingSink::default();
        pump_host_to_controller(
            boxed_source(vec![
                HciPacket::Command(Command::Reset),
                HciPacket::Command(Command::ReadBdAddr),
            ]),
            shared_sink(host_sink.clone()),
            Box::new(controller_sink.clone()),
            Arc::new(BTreeSet::from([Command::Reset.op_code()])),
        )
        .unwrap();

        assert_eq!(
            *controller_sink.0.lock().unwrap(),
            vec![HciPacket::Command(Command::ReadBdAddr)]
        );
        assert!(matches!(
            host_sink.0.lock().unwrap().as_slice(),
            [HciPacket::Event(Event::CommandComplete {
                command_opcode,
                return_parameters: ReturnParameters::Status { status: 0 },
                ..
            })] if *command_opcode == Command::Reset.op_code()
        ));
    }

    #[test]
    fn controller_pump_forwards_packets_to_host() {
        let host_sink = RecordingSink::default();
        let event = HciPacket::Event(Event::CommandComplete {
            num_hci_command_packets: 1,
            command_opcode: Command::Reset.op_code(),
            return_parameters: ReturnParameters::Status { status: 0 },
        });
        pump_controller_to_host(
            boxed_source(vec![event.clone()]),
            shared_sink(host_sink.clone()),
        )
        .unwrap();
        assert_eq!(*host_sink.0.lock().unwrap(), vec![event]);
    }
}
