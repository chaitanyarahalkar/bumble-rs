//! SDP client/server bindings for Classic L2CAP channels.

use bumble_l2cap::{ChannelManager, ClassicChannelState};

use crate::service::{SdpRequestHandler, SdpServer, SdpTransport, TransportError};
use crate::SdpPdu;

/// Server-side SDP endpoint attached to one open Classic L2CAP channel.
#[derive(Debug)]
pub struct SdpL2capServer {
    source_cid: u16,
    server: SdpServer,
}

impl SdpL2capServer {
    pub fn new(source_cid: u16, manager: &ChannelManager) -> Result<Self, TransportError> {
        let channel = open_channel(manager, source_cid)?;
        Ok(Self {
            source_cid,
            server: SdpServer::new(channel.peer_mtu),
        })
    }

    pub fn source_cid(&self) -> u16 {
        self.source_cid
    }

    pub fn server(&self) -> &SdpServer {
        &self.server
    }

    pub fn server_mut(&mut self) -> &mut SdpServer {
        &mut self.server
    }

    /// Answer every pending SDP request SDU and send each response over the
    /// same channel. One L2CAP SDU carries one complete SDP PDU.
    pub fn poll(&mut self, manager: &mut ChannelManager) -> Result<usize, TransportError> {
        let mut processed = 0;
        loop {
            let request_bytes = manager
                .channel_mut(self.source_cid)
                .ok_or_else(|| channel_error(self.source_cid))?
                .pop_received();
            let Some(request_bytes) = request_bytes else {
                return Ok(processed);
            };
            let request = SdpPdu::from_bytes(&request_bytes)
                .map_err(|error| TransportError(format!("invalid SDP request: {error}")))?;
            let response = self.server.handle_request(&request);
            let response_bytes = response
                .to_bytes()
                .map_err(|error| TransportError(format!("invalid SDP response: {error}")))?;
            manager
                .send(self.source_cid, &response_bytes)
                .map_err(|error| TransportError(format!("L2CAP send failed: {error}")))?;
            processed += 1;
        }
    }
}

/// A synchronous [`SdpTransport`] over one Classic L2CAP channel.
///
/// The `drive` callback advances the surrounding host/link until new channel
/// input is available. This keeps the binding independent of a particular
/// executor while allowing the existing continuation-aware [`crate::service::SdpClient`]
/// to retain its simple request/response API.
pub struct L2capSdpTransport<'a, Drive>
where
    Drive: FnMut(&mut ChannelManager) -> Result<(), TransportError>,
{
    manager: &'a mut ChannelManager,
    source_cid: u16,
    drive: Drive,
    drive_limit: usize,
}

impl<'a, Drive> L2capSdpTransport<'a, Drive>
where
    Drive: FnMut(&mut ChannelManager) -> Result<(), TransportError>,
{
    pub fn new(
        manager: &'a mut ChannelManager,
        source_cid: u16,
        drive: Drive,
    ) -> Result<Self, TransportError> {
        open_channel(manager, source_cid)?;
        Ok(Self {
            manager,
            source_cid,
            drive,
            drive_limit: 64,
        })
    }

    pub fn manager_mut(&mut self) -> &mut ChannelManager {
        self.manager
    }
}

impl<Drive> SdpTransport for L2capSdpTransport<'_, Drive>
where
    Drive: FnMut(&mut ChannelManager) -> Result<(), TransportError>,
{
    fn request(&mut self, request: &SdpPdu) -> Result<SdpPdu, TransportError> {
        let bytes = request
            .to_bytes()
            .map_err(|error| TransportError(format!("invalid SDP request: {error}")))?;
        self.manager
            .send(self.source_cid, &bytes)
            .map_err(|error| TransportError(format!("L2CAP send failed: {error}")))?;

        for _ in 0..self.drive_limit {
            (self.drive)(self.manager)?;
            let response = self
                .manager
                .channel_mut(self.source_cid)
                .ok_or_else(|| channel_error(self.source_cid))?
                .pop_received();
            if let Some(response) = response {
                return SdpPdu::from_bytes(&response)
                    .map_err(|error| TransportError(format!("invalid SDP response: {error}")));
            }
        }
        Err(TransportError(
            "SDP response did not arrive before the drive limit".into(),
        ))
    }
}

fn open_channel(
    manager: &ChannelManager,
    source_cid: u16,
) -> Result<&bumble_l2cap::ClassicChannel, TransportError> {
    let channel = manager
        .channel(source_cid)
        .ok_or_else(|| channel_error(source_cid))?;
    if channel.state != ClassicChannelState::Open {
        return Err(channel_error(source_cid));
    }
    Ok(channel)
}

fn channel_error(source_cid: u16) -> TransportError {
    TransportError(format!(
        "Classic L2CAP channel {source_cid:#06x} is not open"
    ))
}
