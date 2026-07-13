use crate::{Error, PacketSink, PacketSource, Result};
use bumble_hci::{Command, Event, HciPacket, ReturnParameters};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandResponse {
    Complete {
        num_hci_command_packets: u8,
        return_parameters: ReturnParameters,
    },
    Status {
        status: u8,
        num_hci_command_packets: u8,
    },
}

impl CommandResponse {
    pub fn status(&self) -> Option<u8> {
        match self {
            Self::Complete {
                return_parameters, ..
            } => return_parameters.status(),
            Self::Status { status, .. } => Some(*status),
        }
    }

    pub fn return_parameters(&self) -> Option<&ReturnParameters> {
        match self {
            Self::Complete {
                return_parameters, ..
            } => Some(return_parameters),
            Self::Status { .. } => None,
        }
    }
}

/// A synchronous one-command-at-a-time HCI request channel.
///
/// Packets unrelated to the command response are retained in arrival order and
/// can be drained with [`take_pending_packets`](Self::take_pending_packets).
pub struct HciCommandChannel<T> {
    transport: T,
    pending_packets: VecDeque<HciPacket>,
}

impl<T> HciCommandChannel<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            pending_packets: VecDeque::new(),
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn take_pending_packets(&mut self) -> Vec<HciPacket> {
        self.pending_packets.drain(..).collect()
    }

    pub fn into_parts(self) -> (T, Vec<HciPacket>) {
        (self.transport, self.pending_packets.into_iter().collect())
    }
}

impl<T: PacketSource + PacketSink> HciCommandChannel<T> {
    pub fn send_command(&mut self, command: Command) -> Result<CommandResponse> {
        let expected_opcode = command.op_code();
        self.transport.write_packet(&HciPacket::Command(command))?;
        self.transport.flush()?;

        loop {
            let packet = self.transport.read_packet()?.ok_or_else(|| {
                Error::Remote(format!(
                    "transport ended before response to HCI command {expected_opcode:#06x}"
                ))
            })?;
            match packet {
                HciPacket::Event(Event::CommandComplete {
                    num_hci_command_packets,
                    command_opcode,
                    return_parameters,
                }) if command_opcode == expected_opcode => {
                    return Ok(CommandResponse::Complete {
                        num_hci_command_packets,
                        return_parameters,
                    });
                }
                HciPacket::Event(Event::CommandStatus {
                    status,
                    num_hci_command_packets,
                    command_opcode,
                }) if command_opcode == expected_opcode => {
                    return Ok(CommandResponse::Status {
                        status,
                        num_hci_command_packets,
                    });
                }
                packet => self.pending_packets.push_back(packet),
            }
        }
    }
}
