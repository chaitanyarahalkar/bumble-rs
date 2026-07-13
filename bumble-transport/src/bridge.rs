use crate::{PacketSink, PacketSource, Result};
use bumble_hci::HciPacket;

/// Direction in which a bridge packet is traveling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeDirection {
    HostToController,
    ControllerToHost,
}

/// Replacement selected by a bridge filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilteredPacket {
    pub packet: HciPacket,
    pub respond_to_sender: bool,
}

impl FilteredPacket {
    pub fn forward(packet: HciPacket) -> Self {
        Self {
            packet,
            respond_to_sender: false,
        }
    }

    pub fn respond(packet: HciPacket) -> Self {
        Self {
            packet,
            respond_to_sender: true,
        }
    }
}

/// A filter may leave a packet unchanged (`Ok(None)`), replace and forward it,
/// or replace it with a response sent back toward its source.
pub type PacketFilter =
    Box<dyn FnMut(&HciPacket) -> Result<Option<FilteredPacket>> + Send + 'static>;

/// Trace callback invoked after filtering and before forwarding.
pub type PacketTrace = Box<dyn FnMut(BridgeDirection, &HciPacket) + Send + 'static>;

/// Bidirectional HCI bridge between host-side and controller-side transports.
///
/// Sources and sinks are separate because several Bumble transports expose
/// split endpoints. Call the directional forwarding methods from the desired
/// scheduling loop; each performs at most one source read.
pub struct HciBridge<HostSource, HostSink, ControllerSource, ControllerSink> {
    host_source: HostSource,
    host_sink: HostSink,
    controller_source: ControllerSource,
    controller_sink: ControllerSink,
    host_to_controller_filter: Option<PacketFilter>,
    controller_to_host_filter: Option<PacketFilter>,
    trace: Option<PacketTrace>,
}

impl<HostSource, HostSink, ControllerSource, ControllerSink>
    HciBridge<HostSource, HostSink, ControllerSource, ControllerSink>
where
    HostSource: PacketSource,
    HostSink: PacketSink,
    ControllerSource: PacketSource,
    ControllerSink: PacketSink,
{
    pub fn new(
        host_source: HostSource,
        host_sink: HostSink,
        controller_source: ControllerSource,
        controller_sink: ControllerSink,
    ) -> Self {
        Self {
            host_source,
            host_sink,
            controller_source,
            controller_sink,
            host_to_controller_filter: None,
            controller_to_host_filter: None,
            trace: None,
        }
    }

    pub fn set_host_to_controller_filter(&mut self, filter: Option<PacketFilter>) {
        self.host_to_controller_filter = filter;
    }

    pub fn set_controller_to_host_filter(&mut self, filter: Option<PacketFilter>) {
        self.controller_to_host_filter = filter;
    }

    pub fn set_trace(&mut self, trace: Option<PacketTrace>) {
        self.trace = trace;
    }

    /// Forward one host packet. Returns `false` at source EOF.
    pub fn forward_host_packet(&mut self) -> Result<bool> {
        let Some(packet) = self.host_source.read_packet()? else {
            return Ok(false);
        };
        let filtered = match self.host_to_controller_filter.as_mut() {
            Some(filter) => filter(&packet)?,
            None => None,
        };
        let (packet, respond_to_sender) = filtered
            .map(|filtered| (filtered.packet, filtered.respond_to_sender))
            .unwrap_or((packet, false));
        if respond_to_sender {
            self.host_sink.write_packet(&packet)?;
            return Ok(true);
        }
        if let Some(trace) = self.trace.as_mut() {
            trace(BridgeDirection::HostToController, &packet);
        }
        self.controller_sink.write_packet(&packet)?;
        Ok(true)
    }

    /// Forward one controller packet. Returns `false` at source EOF.
    pub fn forward_controller_packet(&mut self) -> Result<bool> {
        let Some(packet) = self.controller_source.read_packet()? else {
            return Ok(false);
        };
        let filtered = match self.controller_to_host_filter.as_mut() {
            Some(filter) => filter(&packet)?,
            None => None,
        };
        let (packet, respond_to_sender) = filtered
            .map(|filtered| (filtered.packet, filtered.respond_to_sender))
            .unwrap_or((packet, false));
        if respond_to_sender {
            self.controller_sink.write_packet(&packet)?;
            return Ok(true);
        }
        if let Some(trace) = self.trace.as_mut() {
            trace(BridgeDirection::ControllerToHost, &packet);
        }
        self.host_sink.write_packet(&packet)?;
        Ok(true)
    }

    pub fn into_parts(self) -> (HostSource, HostSink, ControllerSource, ControllerSink) {
        (
            self.host_source,
            self.host_sink,
            self.controller_source,
            self.controller_sink,
        )
    }
}
