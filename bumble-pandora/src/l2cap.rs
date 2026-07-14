use crate::proto::l2cap::connect_request;
use crate::proto::l2cap::connect_response;
use crate::proto::l2cap::disconnect_response;
use crate::proto::l2cap::l2cap_server::L2cap;
use crate::proto::l2cap::receive_request;
use crate::proto::l2cap::send_request;
use crate::proto::l2cap::send_response;
use crate::proto::l2cap::wait_connection_request;
use crate::proto::l2cap::wait_connection_response;
use crate::proto::l2cap::wait_disconnection_response;
use crate::proto::l2cap::{
    Channel, CommandRejectReason, ConnectRequest, ConnectResponse, CreditBasedChannelRequest,
    DisconnectRequest, DisconnectResponse, FixedChannel, ReceiveRequest, ReceiveResponse,
    SendRequest, SendResponse, WaitConnectionRequest, WaitConnectionResponse,
    WaitDisconnectionRequest, WaitDisconnectionResponse,
};
use crate::runtime::{
    handle, L2capChannelEntry, L2capChannelKind, PandoraRuntime, RuntimeState, POLL_INTERVAL,
    PROCEDURE_TIMEOUT,
};
use bumble_l2cap::{
    ClassicChannelSpec, ClassicChannelState, LeCreditBasedChannelSpec, LeCreditBasedChannelState,
};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestedChannel {
    Classic {
        psm: u32,
        spec: ClassicChannelSpec,
    },
    LeCredit {
        psm: u16,
        spec: LeCreditBasedChannelSpec,
        enhanced: bool,
    },
}

impl RequestedChannel {
    fn kind(self) -> L2capChannelKind {
        match self {
            Self::Classic { .. } => L2capChannelKind::Classic,
            Self::LeCredit { .. } => L2capChannelKind::LeCredit,
        }
    }

    fn psm(self) -> u32 {
        match self {
            Self::Classic { psm, .. } => psm,
            Self::LeCredit { psm, .. } => u32::from(psm),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiveSource {
    Dynamic(L2capChannelEntry),
    Fixed { connection_handle: u16, cid: u16 },
}

#[derive(Clone)]
pub struct L2capService {
    runtime: PandoraRuntime,
}

impl L2capService {
    pub fn new(runtime: PandoraRuntime) -> Self {
        Self { runtime }
    }
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn reject_reason(reason: CommandRejectReason) -> i32 {
    reason as i32
}

fn connect_error(reason: CommandRejectReason) -> ConnectResponse {
    ConnectResponse {
        result: Some(connect_response::Result::Error(reject_reason(reason))),
    }
}

fn wait_connection_error(reason: CommandRejectReason) -> WaitConnectionResponse {
    WaitConnectionResponse {
        result: Some(wait_connection_response::Result::Error(reject_reason(
            reason,
        ))),
    }
}

fn disconnect_error(reason: CommandRejectReason) -> DisconnectResponse {
    DisconnectResponse {
        result: Some(disconnect_response::Result::Error(reject_reason(reason))),
    }
}

fn wait_disconnection_error(reason: CommandRejectReason) -> WaitDisconnectionResponse {
    WaitDisconnectionResponse {
        result: Some(wait_disconnection_response::Result::Error(reject_reason(
            reason,
        ))),
    }
}

fn send_error(reason: CommandRejectReason) -> SendResponse {
    SendResponse {
        result: Some(send_response::Result::Error(reject_reason(reason))),
    }
}

fn classic_spec(psm: u32, mtu: u32) -> Result<RequestedChannel, Status> {
    let mtu = u16::try_from(mtu)
        .map_err(|_| Status::invalid_argument("Classic L2CAP MTU exceeds u16"))?;
    Ok(RequestedChannel::Classic {
        psm,
        spec: ClassicChannelSpec { mtu },
    })
}

fn le_spec(request: CreditBasedChannelRequest, enhanced: bool) -> Result<RequestedChannel, Status> {
    let psm = u16::try_from(request.spsm)
        .map_err(|_| Status::invalid_argument("L2CAP SPSM exceeds u16"))?;
    let mtu = u16::try_from(request.mtu)
        .map_err(|_| Status::invalid_argument("credit-based L2CAP MTU exceeds u16"))?;
    let mps = u16::try_from(request.mps)
        .map_err(|_| Status::invalid_argument("credit-based L2CAP MPS exceeds u16"))?;
    let max_credits = u16::try_from(request.initial_credit)
        .map_err(|_| Status::invalid_argument("L2CAP initial credits exceed u16"))?;
    let spec = LeCreditBasedChannelSpec {
        psm: Some(psm),
        mtu,
        mps,
        max_credits,
    };
    spec.validate()
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(RequestedChannel::LeCredit {
        psm,
        spec,
        enhanced,
    })
}

fn connect_spec(request: connect_request::Type) -> Result<RequestedChannel, Status> {
    match request {
        connect_request::Type::Basic(request) => classic_spec(request.psm, request.mtu),
        connect_request::Type::LeCreditBased(request) => le_spec(request, false),
        connect_request::Type::EnhancedCreditBased(request) => le_spec(request, true),
    }
}

fn wait_spec(request: wait_connection_request::Type) -> Result<RequestedChannel, Status> {
    match request {
        wait_connection_request::Type::Basic(request) => classic_spec(request.psm, request.mtu),
        wait_connection_request::Type::LeCreditBased(request) => le_spec(request, false),
        wait_connection_request::Type::EnhancedCreditBased(request) => le_spec(request, true),
    }
}

fn channel_value(entry: L2capChannelEntry) -> Vec<u8> {
    format!(
        "{{\"connection_handle\": {}, \"source_cid\": {}}}",
        entry.connection_handle, entry.source_cid
    )
    .into_bytes()
}

fn register_channel(state: &mut RuntimeState, entry: L2capChannelEntry) -> Channel {
    let value = channel_value(entry);
    state.l2cap_channels.insert(value.clone(), entry);
    Channel {
        cookie: Some(prost_types::Any {
            type_url: String::new(),
            value,
        }),
    }
}

fn channel_key(channel: Option<Channel>) -> Result<Vec<u8>, Status> {
    let value = channel
        .and_then(|channel| channel.cookie)
        .ok_or_else(|| Status::invalid_argument("L2CAP channel cookie is required"))?
        .value;
    if value.is_empty() {
        return Err(Status::invalid_argument(
            "L2CAP channel cookie must not be empty",
        ));
    }
    Ok(value)
}

fn lookup_channel(
    state: &RuntimeState,
    key: &[u8],
) -> Result<L2capChannelEntry, CommandRejectReason> {
    state
        .l2cap_channels
        .get(key)
        .copied()
        .ok_or(CommandRejectReason::InvalidCidInRequest)
}

fn channel_alive(state: &RuntimeState, entry: L2capChannelEntry) -> bool {
    match entry.kind {
        L2capChannelKind::Classic => state
            .device
            .classic_channel(entry.connection_handle, entry.source_cid)
            .is_some_and(|channel| channel.state != ClassicChannelState::Closed),
        L2capChannelKind::LeCredit => state
            .device
            .le_credit_channel(entry.connection_handle, entry.source_cid)
            .is_some_and(|channel| channel.state != LeCreditBasedChannelState::Disconnected),
    }
}

fn collect_incoming(state: &mut RuntimeState) {
    let classic_handles = state
        .device
        .classic_connections()
        .map(|connection| connection.connection_handle)
        .collect::<Vec<_>>();
    for connection_handle in classic_handles {
        for source_cid in state
            .device
            .take_accepted_classic_channels(connection_handle)
        {
            if let Some(channel) = state.device.classic_channel(connection_handle, source_cid) {
                state.pending_l2cap_channels.push(L2capChannelEntry {
                    connection_handle,
                    source_cid,
                    psm: channel.psm,
                    kind: L2capChannelKind::Classic,
                });
            }
        }
    }
    let le_handles = state
        .device
        .le_connections()
        .map(|connection| connection.connection_handle)
        .collect::<Vec<_>>();
    for connection_handle in le_handles {
        for source_cid in state
            .device
            .take_accepted_le_credit_channels(connection_handle)
        {
            if let Some(channel) = state
                .device
                .le_credit_channel(connection_handle, source_cid)
            {
                state.pending_l2cap_channels.push(L2capChannelEntry {
                    connection_handle,
                    source_cid,
                    psm: u32::from(channel.psm),
                    kind: L2capChannelKind::LeCredit,
                });
            }
        }
    }
}

fn take_incoming(
    state: &mut RuntimeState,
    connection_handle: u16,
    request: RequestedChannel,
) -> Option<L2capChannelEntry> {
    collect_incoming(state);
    let position = state.pending_l2cap_channels.iter().position(|entry| {
        entry.connection_handle == connection_handle
            && entry.kind == request.kind()
            && entry.psm == request.psm()
    })?;
    Some(state.pending_l2cap_channels.remove(position))
}

fn register_server(state: &mut RuntimeState, request: RequestedChannel) -> Result<(), Status> {
    match request {
        RequestedChannel::Classic { psm, spec } => {
            if state.l2cap_classic_servers.insert(psm) {
                if let Err(error) = state
                    .device
                    .register_classic_channel_server(Some(psm), spec)
                {
                    state.l2cap_classic_servers.remove(&psm);
                    return Err(Status::invalid_argument(error.to_string()));
                }
            }
        }
        RequestedChannel::LeCredit { psm, spec, .. } => {
            if state.l2cap_le_servers.insert(psm) {
                if let Err(error) = state.device.register_le_credit_server(spec) {
                    state.l2cap_le_servers.remove(&psm);
                    return Err(Status::invalid_argument(error.to_string()));
                }
            }
        }
    }
    Ok(())
}

fn connect_channel(
    state: &mut RuntimeState,
    connection_handle: u16,
    request: RequestedChannel,
) -> Result<Channel, CommandRejectReason> {
    let entry = match request {
        RequestedChannel::Classic { psm, spec } => {
            if state.device.classic_connection(connection_handle).is_none() {
                return Err(CommandRejectReason::CommandNotUnderstood);
            }
            let source_cid = state
                .device
                .connect_classic_channel(&mut state.host, connection_handle, psm, spec)
                .map_err(|_| CommandRejectReason::CommandNotUnderstood)?;
            let deadline = Instant::now() + PROCEDURE_TIMEOUT;
            loop {
                state
                    .poll(POLL_INTERVAL)
                    .map_err(|_| CommandRejectReason::CommandNotUnderstood)?;
                let Some(channel) = state.device.classic_channel(connection_handle, source_cid)
                else {
                    return Err(CommandRejectReason::InvalidCidInRequest);
                };
                match channel.state {
                    ClassicChannelState::Open => break,
                    ClassicChannelState::Closed => {
                        return Err(CommandRejectReason::InvalidCidInRequest);
                    }
                    _ if Instant::now() >= deadline => {
                        return Err(CommandRejectReason::CommandNotUnderstood);
                    }
                    _ => {}
                }
            }
            L2capChannelEntry {
                connection_handle,
                source_cid,
                psm,
                kind: L2capChannelKind::Classic,
            }
        }
        RequestedChannel::LeCredit {
            psm,
            spec,
            enhanced,
        } => {
            if !state.device.is_connected_on_handle(connection_handle) {
                return Err(CommandRejectReason::CommandNotUnderstood);
            }
            let source_cid = if enhanced {
                state
                    .device
                    .connect_enhanced_le_credit_channels(
                        &mut state.host,
                        connection_handle,
                        psm,
                        spec,
                        1,
                    )
                    .map_err(|_| CommandRejectReason::CommandNotUnderstood)?
                    .into_iter()
                    .next()
                    .ok_or(CommandRejectReason::InvalidCidInRequest)?
            } else {
                state
                    .device
                    .connect_le_credit_channel(&mut state.host, connection_handle, psm, spec)
                    .map_err(|_| CommandRejectReason::CommandNotUnderstood)?
            };
            let deadline = Instant::now() + PROCEDURE_TIMEOUT;
            loop {
                state
                    .poll(POLL_INTERVAL)
                    .map_err(|_| CommandRejectReason::CommandNotUnderstood)?;
                if let Some(result) = state
                    .device
                    .le_credit_connection_result(connection_handle, source_cid)
                {
                    if result != 0 {
                        return Err(CommandRejectReason::InvalidCidInRequest);
                    }
                    if state
                        .device
                        .le_credit_channel(connection_handle, source_cid)
                        .is_some_and(|channel| {
                            channel.state == LeCreditBasedChannelState::Connected
                        })
                    {
                        break;
                    }
                }
                if Instant::now() >= deadline {
                    return Err(CommandRejectReason::CommandNotUnderstood);
                }
            }
            L2capChannelEntry {
                connection_handle,
                source_cid,
                psm: u32::from(psm),
                kind: L2capChannelKind::LeCredit,
            }
        }
    };
    Ok(register_channel(state, entry))
}

fn receive_source(state: &RuntimeState, request: ReceiveRequest) -> Result<ReceiveSource, Status> {
    match request.source {
        Some(receive_request::Source::Channel(channel)) => {
            let key = channel_key(Some(channel))?;
            lookup_channel(state, &key)
                .map(ReceiveSource::Dynamic)
                .map_err(|_| Status::invalid_argument("unknown L2CAP channel cookie"))
        }
        Some(receive_request::Source::FixedChannel(channel)) => fixed_source(state, channel),
        None => Err(Status::invalid_argument("L2CAP receive source is required")),
    }
}

fn fixed_source(state: &RuntimeState, channel: FixedChannel) -> Result<ReceiveSource, Status> {
    let connection_handle = handle(channel.connection)?;
    if !state.connection_exists(connection_handle) {
        return Err(Status::invalid_argument(
            "fixed-channel connection is not active",
        ));
    }
    let cid = u16::try_from(channel.cid)
        .map_err(|_| Status::invalid_argument("fixed L2CAP CID exceeds u16"))?;
    Ok(ReceiveSource::Fixed {
        connection_handle,
        cid,
    })
}

#[tonic::async_trait]
impl L2cap for L2capService {
    async fn connect(
        &self,
        request: Request<ConnectRequest>,
    ) -> Result<Response<ConnectResponse>, Status> {
        let request = request.into_inner();
        let connection_handle = handle(request.connection)?;
        let spec = connect_spec(
            request
                .r#type
                .ok_or_else(|| Status::invalid_argument("L2CAP channel type is required"))?,
        )?;
        let response = self
            .runtime
            .blocking(move |state| {
                Ok(match connect_channel(state, connection_handle, spec) {
                    Ok(channel) => ConnectResponse {
                        result: Some(connect_response::Result::Channel(channel)),
                    },
                    Err(reason) => connect_error(reason),
                })
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn wait_connection(
        &self,
        request: Request<WaitConnectionRequest>,
    ) -> Result<Response<WaitConnectionResponse>, Status> {
        let request = request.into_inner();
        let connection_handle = handle(request.connection)?;
        let spec = wait_spec(
            request
                .r#type
                .ok_or_else(|| Status::invalid_argument("L2CAP channel type is required"))?,
        )?;
        let response = self
            .runtime
            .blocking(move |state| {
                let valid_connection = match spec.kind() {
                    L2capChannelKind::Classic => {
                        state.device.classic_connection(connection_handle).is_some()
                    }
                    L2capChannelKind::LeCredit => {
                        state.device.is_connected_on_handle(connection_handle)
                    }
                };
                if !valid_connection {
                    return Err(Status::invalid_argument(
                        "the specified L2CAP connection is not active",
                    ));
                }
                register_server(state, spec)?;
                let deadline = Instant::now() + PROCEDURE_TIMEOUT;
                loop {
                    state.poll(POLL_INTERVAL).map_err(internal)?;
                    if let Some(entry) = take_incoming(state, connection_handle, spec) {
                        return Ok(WaitConnectionResponse {
                            result: Some(wait_connection_response::Result::Channel(
                                register_channel(state, entry),
                            )),
                        });
                    }
                    if Instant::now() >= deadline {
                        return Ok(wait_connection_error(
                            CommandRejectReason::CommandNotUnderstood,
                        ));
                    }
                }
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn disconnect(
        &self,
        request: Request<DisconnectRequest>,
    ) -> Result<Response<DisconnectResponse>, Status> {
        let key = channel_key(request.into_inner().channel)?;
        let response = self
            .runtime
            .blocking(move |state| {
                let entry = match lookup_channel(state, &key) {
                    Ok(entry) => entry,
                    Err(reason) => return Ok(disconnect_error(reason)),
                };
                let result = match entry.kind {
                    L2capChannelKind::Classic => state.device.disconnect_classic_channel(
                        &mut state.host,
                        entry.connection_handle,
                        entry.source_cid,
                    ),
                    L2capChannelKind::LeCredit => state.device.disconnect_le_credit_channel(
                        &mut state.host,
                        entry.connection_handle,
                        entry.source_cid,
                    ),
                };
                if result.is_err() {
                    return Ok(disconnect_error(CommandRejectReason::CommandNotUnderstood));
                }
                let deadline = Instant::now() + PROCEDURE_TIMEOUT;
                while channel_alive(state, entry) {
                    if Instant::now() >= deadline {
                        return Ok(disconnect_error(CommandRejectReason::CommandNotUnderstood));
                    }
                    state.poll(POLL_INTERVAL).map_err(internal)?;
                }
                Ok(DisconnectResponse {
                    result: Some(disconnect_response::Result::Success(())),
                })
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn wait_disconnection(
        &self,
        request: Request<WaitDisconnectionRequest>,
    ) -> Result<Response<WaitDisconnectionResponse>, Status> {
        let key = channel_key(request.into_inner().channel)?;
        let response = self
            .runtime
            .blocking(move |state| {
                let entry = match lookup_channel(state, &key) {
                    Ok(entry) => entry,
                    Err(reason) => return Ok(wait_disconnection_error(reason)),
                };
                while channel_alive(state, entry) {
                    state.poll(POLL_INTERVAL).map_err(internal)?;
                }
                Ok(WaitDisconnectionResponse {
                    result: Some(wait_disconnection_response::Result::Success(())),
                })
            })
            .await?;
        Ok(Response::new(response))
    }

    type ReceiveStream = ResponseStream<ReceiveResponse>;

    async fn receive(
        &self,
        request: Request<ReceiveRequest>,
    ) -> Result<Response<Self::ReceiveStream>, Status> {
        let request = request.into_inner();
        let source = self
            .runtime
            .blocking(move |state| receive_source(state, request))
            .await?;
        let state = Arc::clone(&self.runtime.state);
        let (sender, receiver) = mpsc::channel(64);
        tokio::task::spawn_blocking(move || {
            while !sender.is_closed() {
                let (sdus, alive) = {
                    let mut state = match state.lock() {
                        Ok(state) => state,
                        Err(_) => return,
                    };
                    if let Err(error) = state.poll(POLL_INTERVAL) {
                        let _ = sender.blocking_send(Err(internal(error)));
                        return;
                    }
                    match source {
                        ReceiveSource::Dynamic(entry) => {
                            let sdus = match entry.kind {
                                L2capChannelKind::Classic => {
                                    state.device.take_classic_channel_sdus(
                                        entry.connection_handle,
                                        entry.source_cid,
                                    )
                                }
                                L2capChannelKind::LeCredit => state
                                    .device
                                    .take_le_credit_sdus(entry.connection_handle, entry.source_cid),
                            };
                            (sdus, channel_alive(&state, entry))
                        }
                        ReceiveSource::Fixed {
                            connection_handle,
                            cid,
                        } => (
                            state.device.take_l2cap_on_handle(connection_handle, cid),
                            state.connection_exists(connection_handle),
                        ),
                    }
                };
                for data in sdus {
                    if sender.blocking_send(Ok(ReceiveResponse { data })).is_err() {
                        return;
                    }
                }
                if !alive {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    async fn send(&self, request: Request<SendRequest>) -> Result<Response<SendResponse>, Status> {
        let request = request.into_inner();
        let sink = request
            .sink
            .ok_or_else(|| Status::invalid_argument("L2CAP send sink is required"))?;
        let data = request.data;
        let response = self
            .runtime
            .blocking(move |state| {
                let result = match sink {
                    send_request::Sink::Channel(channel) => {
                        let key = channel_key(Some(channel))?;
                        let entry = match lookup_channel(state, &key) {
                            Ok(entry) => entry,
                            Err(reason) => return Ok(send_error(reason)),
                        };
                        match entry.kind {
                            L2capChannelKind::Classic => state.device.send_classic_channel_sdu(
                                &mut state.host,
                                entry.connection_handle,
                                entry.source_cid,
                                &data,
                            ),
                            L2capChannelKind::LeCredit => state.device.send_le_credit_sdu(
                                &mut state.host,
                                entry.connection_handle,
                                entry.source_cid,
                                &data,
                            ),
                        }
                    }
                    send_request::Sink::FixedChannel(channel) => {
                        let ReceiveSource::Fixed {
                            connection_handle,
                            cid,
                        } = fixed_source(state, channel)?
                        else {
                            unreachable!();
                        };
                        if state.device.send_l2cap_on_handle(
                            &mut state.host,
                            connection_handle,
                            cid,
                            &data,
                        ) {
                            Ok(())
                        } else {
                            Err(bumble_l2cap::Error::InvalidPacket(
                                "failed to send fixed-channel L2CAP data".into(),
                            ))
                        }
                    }
                };
                Ok(if result.is_ok() {
                    SendResponse {
                        result: Some(send_response::Result::Success(())),
                    }
                } else {
                    send_error(CommandRejectReason::CommandNotUnderstood)
                })
            })
            .await?;
        Ok(Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_channel_cookie_matches_upstream_json_shape() {
        let entry = L2capChannelEntry {
            connection_handle: 0x1234,
            source_cid: 0x0040,
            psm: 0x1001,
            kind: L2capChannelKind::Classic,
        };
        assert_eq!(
            channel_value(entry),
            br#"{"connection_handle": 4660, "source_cid": 64}"#
        );
    }

    #[test]
    fn request_specs_reject_values_that_cannot_reach_the_wire() {
        assert!(classic_spec(0x1001, u32::from(u16::MAX) + 1).is_err());
        assert!(le_spec(
            CreditBasedChannelRequest {
                spsm: 0x80,
                mtu: 22,
                mps: 23,
                initial_credit: 1,
            },
            false,
        )
        .is_err());
    }
}
