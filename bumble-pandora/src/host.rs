use crate::data_types;
use crate::proto::advertise_request;
use crate::proto::connect_le_request;
use crate::proto::connect_le_response;
use crate::proto::connect_response;
use crate::proto::host_server::Host;
use crate::proto::scanning_response;
use crate::proto::wait_connection_response;
use crate::proto::{
    AdvertiseRequest, AdvertiseResponse, ConnectLeRequest, ConnectLeResponse, ConnectRequest,
    ConnectResponse, ConnectabilityMode, DisconnectRequest, DiscoverabilityMode,
    GetConnectionParametersRequest, GetConnectionParametersResponse, InquiryResponse, PrimaryPhy,
    ReadLocalAddressResponse, ScanRequest, ScanningResponse, SecondaryPhy,
    SetConnectabilityModeRequest, SetDiscoverabilityModeRequest, WaitConnectionRequest,
    WaitConnectionResponse, WaitConnectionUpdateRequest, WaitConnectionUpdateResponse,
    WaitDisconnectionRequest,
};
use crate::runtime::{
    address, cookie, handle, successful_command, PandoraRuntime, RuntimeState, POLL_INTERVAL,
    PROCEDURE_TIMEOUT,
};
use bumble::{Address, AddressType};
use bumble_hci::{AdvertisingReport, Command, ExtendedAdvertisingReport};
use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub struct HostService {
    runtime: PandoraRuntime,
    shutdown: Option<watch::Sender<bool>>,
}

impl HostService {
    pub fn new(runtime: PandoraRuntime) -> Self {
        Self {
            runtime,
            shutdown: None,
        }
    }

    pub fn with_shutdown(runtime: PandoraRuntime, shutdown: watch::Sender<bool>) -> Self {
        Self {
            runtime,
            shutdown: Some(shutdown),
        }
    }
}

fn internal(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn scan_interval(value: f32) -> u16 {
    if value <= 0.0 || !value.is_finite() {
        0x0010
    } else {
        (value / 0.625).round().clamp(4.0, 65_535.0) as u16
    }
}

#[cfg(test)]
fn address_variant(address: &Address) -> scanning_response::Address {
    let bytes = address.address_bytes().to_vec();
    match address.address_type() {
        AddressType::PUBLIC_DEVICE => scanning_response::Address::Public(bytes),
        AddressType::PUBLIC_IDENTITY => scanning_response::Address::PublicIdentity(bytes),
        AddressType::RANDOM_IDENTITY => scanning_response::Address::RandomStaticIdentity(bytes),
        _ => scanning_response::Address::Random(bytes),
    }
}

fn legacy_response(report: AdvertisingReport) -> ScanningResponse {
    let address = match report.address_type {
        0 => scanning_response::Address::Public(report.address.address_bytes().to_vec()),
        2 => scanning_response::Address::PublicIdentity(report.address.address_bytes().to_vec()),
        3 => scanning_response::Address::RandomStaticIdentity(
            report.address.address_bytes().to_vec(),
        ),
        _ => scanning_response::Address::Random(report.address.address_bytes().to_vec()),
    };
    ScanningResponse {
        legacy: true,
        connectable: matches!(report.event_type, 0 | 1),
        scannable: matches!(report.event_type, 0 | 2),
        truncated: false,
        sid: 0,
        primary_phy: PrimaryPhy::Primary1m as i32,
        secondary_phy: SecondaryPhy::SecondaryNone as i32,
        tx_power: 0,
        rssi: i32::from(report.rssi),
        periodic_advertising_interval: 0.0,
        data: Some(data_types::pack(&report.data)),
        address: Some(address),
        direct_address: None,
    }
}

fn extended_response(report: ExtendedAdvertisingReport) -> ScanningResponse {
    let direct_address = if report.direct_address.address_bytes() == &[0; 6] {
        None
    } else {
        let bytes = report.direct_address.address_bytes().to_vec();
        Some(match report.direct_address_type {
            0 => scanning_response::DirectAddress::DirectPublic(bytes),
            2 => scanning_response::DirectAddress::DirectResolvedPublic(bytes),
            3 => scanning_response::DirectAddress::DirectResolvedRandom(bytes),
            0xFE => scanning_response::DirectAddress::DirectUnresolvedRandom(bytes),
            _ => scanning_response::DirectAddress::DirectNonResolvableRandom(bytes),
        })
    };
    ScanningResponse {
        legacy: report.event_type & 0x10 != 0,
        connectable: report.event_type & 0x01 != 0,
        scannable: report.event_type & 0x02 != 0,
        truncated: (report.event_type >> 5) & 0x03 == 0x02,
        sid: u32::from(report.advertising_sid),
        primary_phy: if report.primary_phy == 3 {
            PrimaryPhy::PrimaryCoded as i32
        } else {
            PrimaryPhy::Primary1m as i32
        },
        secondary_phy: match report.secondary_phy {
            1 => SecondaryPhy::Secondary1m as i32,
            2 => SecondaryPhy::Secondary2m as i32,
            3 => SecondaryPhy::SecondaryCoded as i32,
            _ => SecondaryPhy::SecondaryNone as i32,
        },
        tx_power: i32::from(report.tx_power),
        rssi: i32::from(report.rssi),
        periodic_advertising_interval: f32::from(report.periodic_advertising_interval) * 1.25,
        data: Some(data_types::pack(&report.data)),
        address: Some(match report.address_type {
            0 => scanning_response::Address::Public(report.address.address_bytes().to_vec()),
            2 => {
                scanning_response::Address::PublicIdentity(report.address.address_bytes().to_vec())
            }
            3 => scanning_response::Address::RandomStaticIdentity(
                report.address.address_bytes().to_vec(),
            ),
            _ => scanning_response::Address::Random(report.address.address_bytes().to_vec()),
        }),
        direct_address,
    }
}

fn start_scan(state: &mut RuntimeState, request: &ScanRequest) -> Result<(), Status> {
    let interval = scan_interval(request.interval);
    let window = if request.window <= 0.0 {
        interval
    } else {
        scan_interval(request.window).min(interval)
    };
    let own_address_type = u8::try_from(request.own_address_type)
        .map_err(|_| Status::invalid_argument("own address type is out of range"))?;
    if request.legacy {
        successful_command(
            &mut state.host,
            Command::LeSetScanParameters {
                le_scan_type: u8::from(!request.passive),
                le_scan_interval: interval,
                le_scan_window: window,
                own_address_type,
                scanning_filter_policy: 0,
            },
            "setting legacy scan parameters",
        )
        .map_err(internal)?;
        successful_command(
            &mut state.host,
            Command::LeSetScanEnable {
                le_scan_enable: 1,
                filter_duplicates: 0,
            },
            "starting legacy scan",
        )
        .map_err(internal)
    } else {
        let mut scanning_phys = 0u8;
        if request.phys.is_empty() || request.phys.contains(&(PrimaryPhy::Primary1m as i32)) {
            scanning_phys |= 0x01;
        }
        if request.phys.is_empty() || request.phys.contains(&(PrimaryPhy::PrimaryCoded as i32)) {
            scanning_phys |= 0x04;
        }
        let count = scanning_phys.count_ones() as usize;
        successful_command(
            &mut state.host,
            Command::LeSetExtendedScanParameters {
                own_address_type,
                scanning_filter_policy: 0,
                scanning_phys,
                scan_types: vec![u8::from(!request.passive); count],
                scan_intervals: vec![interval; count],
                scan_windows: vec![window; count],
            },
            "setting extended scan parameters",
        )
        .map_err(internal)?;
        successful_command(
            &mut state.host,
            Command::LeSetExtendedScanEnable {
                enable: 1,
                filter_duplicates: 0,
                duration: 0,
                period: 0,
            },
            "starting extended scan",
        )
        .map_err(internal)
    }
}

fn stop_scan(state: &mut RuntimeState, legacy: bool) {
    let command = if legacy {
        Command::LeSetScanEnable {
            le_scan_enable: 0,
            filter_duplicates: 0,
        }
    } else {
        Command::LeSetExtendedScanEnable {
            enable: 0,
            filter_duplicates: 0,
            duration: 0,
            period: 0,
        }
    };
    let _ = successful_command(&mut state.host, command, "stopping scan");
}

fn extended_data_command(
    state: &mut RuntimeState,
    scan_response: bool,
    data: &[u8],
) -> Result<(), Status> {
    let chunks = if data.is_empty() {
        vec![&[][..]]
    } else {
        data.chunks(251).collect::<Vec<_>>()
    };
    let last = chunks.len().saturating_sub(1);
    for (index, chunk) in chunks.into_iter().enumerate() {
        let operation = match (index, last) {
            (0, 0) => 0x03,
            (0, _) => 0x01,
            (index, last) if index == last => 0x02,
            _ => 0x00,
        };
        let command = if scan_response {
            Command::LeSetExtendedScanResponseData {
                advertising_handle: 0,
                operation,
                fragment_preference: 0,
                scan_response_data: chunk.to_vec(),
            }
        } else {
            Command::LeSetExtendedAdvertisingData {
                advertising_handle: 0,
                operation,
                fragment_preference: 0,
                advertising_data: chunk.to_vec(),
            }
        };
        successful_command(
            &mut state.host,
            command,
            "setting extended advertising data",
        )
        .map_err(internal)?;
    }
    Ok(())
}

fn start_advertising(state: &mut RuntimeState, request: &AdvertiseRequest) -> Result<(), Status> {
    let data = request
        .data
        .as_ref()
        .map(|data| data_types::unpack(data, &state.config.name, state.config.class_of_device))
        .transpose()?
        .unwrap_or_default();
    let scan_response = request
        .scan_response_data
        .as_ref()
        .map(|data| data_types::unpack(data, &state.config.name, state.config.class_of_device))
        .transpose()?
        .unwrap_or_default();
    let interval = if request.interval <= 0.0 || !request.interval.is_finite() {
        0x0800
    } else {
        request.interval.round().clamp(0x20 as f32, 0x4000 as f32) as u16
    };
    let interval_max = (f32::from(interval) + request.interval_range.max(0.0))
        .round()
        .clamp(f32::from(interval), 0x4000 as f32) as u16;
    let (peer_address, peer_address_type, directed) = match request.target.as_ref() {
        Some(advertise_request::Target::Public(bytes)) => {
            (address(bytes.clone(), AddressType::PUBLIC_DEVICE)?, 0, true)
        }
        Some(advertise_request::Target::Random(bytes)) => {
            (address(bytes.clone(), AddressType::RANDOM_DEVICE)?, 1, true)
        }
        None => (
            Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE),
            0,
            false,
        ),
    };
    let own_address_type = u8::try_from(request.own_address_type)
        .map_err(|_| Status::invalid_argument("own address type is out of range"))?;
    if request.legacy {
        if data.len() > 31 || scan_response.len() > 31 {
            return Err(Status::invalid_argument(
                "legacy advertising and scan-response data must not exceed 31 bytes",
            ));
        }
        let advertising_type = if directed {
            1
        } else if request.connectable {
            0
        } else if !scan_response.is_empty() {
            2
        } else {
            3
        };
        successful_command(
            &mut state.host,
            Command::LeSetAdvertisingParameters {
                advertising_interval_min: interval,
                advertising_interval_max: interval_max,
                advertising_type,
                own_address_type,
                peer_address_type,
                peer_address,
                advertising_channel_map: 7,
                advertising_filter_policy: 0,
            },
            "setting legacy advertising parameters",
        )
        .map_err(internal)?;
        successful_command(
            &mut state.host,
            Command::LeSetAdvertisingData {
                advertising_data: data,
            },
            "setting legacy advertising data",
        )
        .map_err(internal)?;
        successful_command(
            &mut state.host,
            Command::LeSetScanResponseData {
                scan_response_data: scan_response,
            },
            "setting legacy scan-response data",
        )
        .map_err(internal)?;
        successful_command(
            &mut state.host,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 1,
            },
            "starting legacy advertising",
        )
        .map_err(internal)
    } else {
        let mut properties = 0u16;
        properties |= u16::from(request.connectable);
        properties |= u16::from(!scan_response.is_empty()) << 1;
        properties |= u16::from(directed) << 2;
        let primary_phy = if request.primary_phy == PrimaryPhy::PrimaryCoded as i32 {
            3
        } else {
            1
        };
        let secondary_phy = match SecondaryPhy::try_from(request.secondary_phy) {
            Ok(SecondaryPhy::Secondary2m) => 2,
            Ok(SecondaryPhy::SecondaryCoded) => 3,
            _ => 1,
        };
        successful_command(
            &mut state.host,
            Command::LeSetExtendedAdvertisingParameters {
                advertising_handle: 0,
                advertising_event_properties: properties,
                primary_advertising_interval_min: u32::from(interval),
                primary_advertising_interval_max: u32::from(interval_max),
                primary_advertising_channel_map: 7,
                own_address_type,
                peer_address_type,
                peer_address,
                advertising_filter_policy: 0,
                advertising_tx_power: 0x7F,
                primary_advertising_phy: primary_phy,
                secondary_advertising_max_skip: 0,
                secondary_advertising_phy: secondary_phy,
                advertising_sid: 0,
                scan_request_notification_enable: 0,
            },
            "setting extended advertising parameters",
        )
        .map_err(internal)?;
        extended_data_command(state, false, &data)?;
        extended_data_command(state, true, &scan_response)?;
        successful_command(
            &mut state.host,
            Command::LeSetExtendedAdvertisingEnable {
                enable: 1,
                advertising_handles: vec![0],
                durations: vec![0],
                max_extended_advertising_events: vec![0],
            },
            "starting extended advertising",
        )
        .map_err(internal)
    }
}

fn stop_advertising(state: &mut RuntimeState, legacy: bool) {
    let command = if legacy {
        Command::LeSetAdvertisingEnable {
            advertising_enable: 0,
        }
    } else {
        Command::LeSetExtendedAdvertisingEnable {
            enable: 0,
            advertising_handles: vec![0],
            durations: vec![0],
            max_extended_advertising_events: vec![0],
        }
    };
    let _ = successful_command(&mut state.host, command, "stopping advertising");
}

#[tonic::async_trait]
impl Host for HostService {
    async fn factory_reset(&self, _: Request<()>) -> Result<Response<()>, Status> {
        self.runtime
            .blocking(|state| state.reset().map_err(internal))
            .await?;
        if let Some(shutdown) = self.shutdown.clone() {
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                let _ = shutdown.send(true);
            });
        }
        Ok(Response::new(()))
    }

    async fn reset(&self, _: Request<()>) -> Result<Response<()>, Status> {
        self.runtime
            .blocking(|state| state.reset().map_err(internal))
            .await?;
        Ok(Response::new(()))
    }

    async fn read_local_address(
        &self,
        _: Request<()>,
    ) -> Result<Response<ReadLocalAddressResponse>, Status> {
        let address = self
            .runtime
            .blocking(|state| Ok(state.public_address.address_bytes().to_vec()))
            .await?;
        Ok(Response::new(ReadLocalAddressResponse { address }))
    }

    async fn connect(
        &self,
        request: Request<ConnectRequest>,
    ) -> Result<Response<ConnectResponse>, Status> {
        let peer = address(request.into_inner().address, AddressType::PUBLIC_DEVICE)?;
        let response = self
            .runtime
            .blocking(move |state| {
                if state
                    .device
                    .classic_connection_handle_for_peer(&peer)
                    .is_some()
                {
                    return Ok(ConnectResponse {
                        result: Some(connect_response::Result::ConnectionAlreadyExists(())),
                    });
                }
                state.device.connect_classic(&mut state.host, peer.clone());
                Ok(match state.wait_for_classic_connection(&peer) {
                    Ok(handle) => ConnectResponse {
                        result: Some(connect_response::Result::Connection(cookie(handle))),
                    },
                    Err(_) => ConnectResponse {
                        result: Some(connect_response::Result::PeerNotFound(())),
                    },
                })
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn wait_connection(
        &self,
        request: Request<WaitConnectionRequest>,
    ) -> Result<Response<WaitConnectionResponse>, Status> {
        let peer = address(request.into_inner().address, AddressType::PUBLIC_DEVICE)?;
        let handle = self
            .runtime
            .blocking(move |state| {
                let deadline = Instant::now() + PROCEDURE_TIMEOUT;
                loop {
                    state.device.poll(&mut state.host);
                    if let Some(handle) = state.device.classic_connection_handle_for_peer(&peer) {
                        if state.waited_classic_connections.insert(handle) {
                            return Ok(handle);
                        }
                    }
                    for pending in state.device.take_classic_connection_requests() {
                        if pending == peer {
                            state.device.accept_classic(&mut state.host, pending);
                        }
                    }
                    if Instant::now() >= deadline {
                        return Err(Status::deadline_exceeded(
                            "timed out waiting for Classic connection",
                        ));
                    }
                    if !state.poll(POLL_INTERVAL).map_err(internal)? {
                        return Err(Status::unavailable("HCI transport ended"));
                    }
                }
            })
            .await?;
        Ok(Response::new(WaitConnectionResponse {
            result: Some(wait_connection_response::Result::Connection(cookie(handle))),
        }))
    }

    async fn connect_le(
        &self,
        request: Request<ConnectLeRequest>,
    ) -> Result<Response<ConnectLeResponse>, Status> {
        let request = request.into_inner();
        let (bytes, address_type) = match request.address {
            Some(connect_le_request::Address::Public(bytes)) => (bytes, AddressType::PUBLIC_DEVICE),
            Some(connect_le_request::Address::Random(bytes)) => (bytes, AddressType::RANDOM_DEVICE),
            Some(connect_le_request::Address::PublicIdentity(bytes)) => {
                (bytes, AddressType::PUBLIC_IDENTITY)
            }
            Some(connect_le_request::Address::RandomStaticIdentity(bytes)) => {
                (bytes, AddressType::RANDOM_IDENTITY)
            }
            None => return Err(Status::invalid_argument("peer address is required")),
        };
        let peer = address(bytes, address_type)?;
        let own_address_type = u8::try_from(request.own_address_type)
            .map_err(|_| Status::invalid_argument("own address type is out of range"))?;
        let response = self
            .runtime
            .blocking(move |state| {
                if state.device.connection_handle_for_peer(&peer).is_some() {
                    return Ok(ConnectLeResponse {
                        result: Some(connect_le_response::Result::ConnectionAlreadyExists(())),
                    });
                }
                successful_command(
                    &mut state.host,
                    Command::LeCreateConnection {
                        le_scan_interval: 0x0010,
                        le_scan_window: 0x0010,
                        initiator_filter_policy: 0,
                        peer_address_type: u8::from(!peer.is_public()),
                        peer_address: peer.clone(),
                        own_address_type,
                        connection_interval_min: 24,
                        connection_interval_max: 40,
                        max_latency: 0,
                        supervision_timeout: 42,
                        min_ce_length: 0,
                        max_ce_length: 0,
                    },
                    "creating LE connection",
                )
                .map_err(internal)?;
                Ok(match state.wait_for_le_connection(Some(&peer)) {
                    Ok(handle) => ConnectLeResponse {
                        result: Some(connect_le_response::Result::Connection(cookie(handle))),
                    },
                    Err(_) => ConnectLeResponse {
                        result: Some(connect_le_response::Result::PeerNotFound(())),
                    },
                })
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn wait_connection_update(
        &self,
        _: Request<WaitConnectionUpdateRequest>,
    ) -> Result<Response<WaitConnectionUpdateResponse>, Status> {
        Err(Status::unimplemented(
            "upstream Bumble does not implement WaitConnectionUpdate",
        ))
    }

    async fn get_connection_parameters(
        &self,
        _: Request<GetConnectionParametersRequest>,
    ) -> Result<Response<GetConnectionParametersResponse>, Status> {
        Err(Status::unimplemented(
            "upstream Bumble does not implement GetConnectionParameters",
        ))
    }

    async fn disconnect(
        &self,
        request: Request<DisconnectRequest>,
    ) -> Result<Response<()>, Status> {
        let handle = handle(request.into_inner().connection)?;
        self.runtime
            .blocking(move |state| {
                if state.connection_exists(handle) {
                    state
                        .device
                        .disconnect_handle(&mut state.host, handle, 0x13);
                    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
                    while state.connection_exists(handle) {
                        if Instant::now() >= deadline {
                            return Err(Status::deadline_exceeded(
                                "timed out waiting for disconnection",
                            ));
                        }
                        if !state.poll(POLL_INTERVAL).map_err(internal)? {
                            return Err(Status::unavailable("HCI transport ended"));
                        }
                    }
                }
                Ok(())
            })
            .await?;
        Ok(Response::new(()))
    }

    async fn wait_disconnection(
        &self,
        request: Request<WaitDisconnectionRequest>,
    ) -> Result<Response<()>, Status> {
        let handle = handle(request.into_inner().connection)?;
        self.runtime
            .blocking(move |state| {
                while state.connection_exists(handle) {
                    if !state.poll(POLL_INTERVAL).map_err(internal)? {
                        return Err(Status::unavailable("HCI transport ended"));
                    }
                }
                Ok(())
            })
            .await?;
        Ok(Response::new(()))
    }

    type AdvertiseStream = ResponseStream<AdvertiseResponse>;

    async fn advertise(
        &self,
        request: Request<AdvertiseRequest>,
    ) -> Result<Response<Self::AdvertiseStream>, Status> {
        let request = request.into_inner();
        let state = Arc::clone(&self.runtime.state);
        let (sender, receiver) = mpsc::channel(16);
        tokio::task::spawn_blocking(move || {
            let mut known = BTreeSet::new();
            {
                let mut state = match state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                if let Err(error) = start_advertising(&mut state, &request) {
                    let _ = sender.blocking_send(Err(error));
                    return;
                }
                known.extend(
                    state
                        .device
                        .le_connections()
                        .map(|connection| connection.connection_handle),
                );
            }
            while !sender.is_closed() {
                let connections = {
                    let mut state = match state.lock() {
                        Ok(state) => state,
                        Err(_) => break,
                    };
                    if let Err(error) = state.poll(POLL_INTERVAL) {
                        let _ = sender.blocking_send(Err(internal(error)));
                        break;
                    }
                    state
                        .device
                        .le_connections()
                        .filter(|connection| connection.role == 1)
                        .map(|connection| connection.connection_handle)
                        .filter(|handle| known.insert(*handle))
                        .collect::<Vec<_>>()
                };
                for handle in connections {
                    if sender
                        .blocking_send(Ok(AdvertiseResponse {
                            connection: Some(cookie(handle)),
                        }))
                        .is_err()
                    {
                        break;
                    }
                    if request.connectable {
                        let mut state = match state.lock() {
                            Ok(state) => state,
                            Err(_) => break,
                        };
                        let command = if request.legacy {
                            Command::LeSetAdvertisingEnable {
                                advertising_enable: 1,
                            }
                        } else {
                            Command::LeSetExtendedAdvertisingEnable {
                                enable: 1,
                                advertising_handles: vec![0],
                                durations: vec![0],
                                max_extended_advertising_events: vec![0],
                            }
                        };
                        let _ = successful_command(
                            &mut state.host,
                            command,
                            "restarting connectable advertising",
                        );
                    }
                }
            }
            if let Ok(mut state) = state.lock() {
                stop_advertising(&mut state, request.legacy);
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    type ScanStream = ResponseStream<ScanningResponse>;

    async fn scan(
        &self,
        request: Request<ScanRequest>,
    ) -> Result<Response<Self::ScanStream>, Status> {
        let request = request.into_inner();
        let state = Arc::clone(&self.runtime.state);
        let (sender, receiver) = mpsc::channel(64);
        tokio::task::spawn_blocking(move || {
            {
                let mut state = match state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                if let Err(error) = start_scan(&mut state, &request) {
                    let _ = sender.blocking_send(Err(error));
                    return;
                }
            }
            while !sender.is_closed() {
                let responses = {
                    let mut state = match state.lock() {
                        Ok(state) => state,
                        Err(_) => break,
                    };
                    if let Err(error) = state.poll(POLL_INTERVAL) {
                        let _ = sender.blocking_send(Err(internal(error)));
                        break;
                    }
                    let mut responses = state
                        .device
                        .take_advertising_reports()
                        .into_iter()
                        .map(legacy_response)
                        .collect::<Vec<_>>();
                    responses.extend(
                        state
                            .device
                            .take_extended_advertising_reports()
                            .into_iter()
                            .map(extended_response),
                    );
                    responses
                };
                for response in responses {
                    if sender.blocking_send(Ok(response)).is_err() {
                        break;
                    }
                }
            }
            if let Ok(mut state) = state.lock() {
                stop_scan(&mut state, request.legacy);
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    type InquiryStream = ResponseStream<InquiryResponse>;

    async fn inquiry(&self, _: Request<()>) -> Result<Response<Self::InquiryStream>, Status> {
        let state = Arc::clone(&self.runtime.state);
        let (sender, receiver) = mpsc::channel(32);
        tokio::task::spawn_blocking(move || {
            {
                let mut state = match state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                if let Err(error) = successful_command(
                    &mut state.host,
                    Command::Inquiry {
                        lap: 0x9E8B33,
                        inquiry_length: 0x30,
                        num_responses: 0,
                    },
                    "starting inquiry",
                ) {
                    let _ = sender.blocking_send(Err(internal(error)));
                    return;
                }
            }
            let mut complete = false;
            while !complete && !sender.is_closed() {
                let (responses, statuses) = {
                    let mut state = match state.lock() {
                        Ok(state) => state,
                        Err(_) => break,
                    };
                    if let Err(error) = state.poll(POLL_INTERVAL) {
                        let _ = sender.blocking_send(Err(internal(error)));
                        break;
                    }
                    let responses = state
                        .device
                        .take_classic_inquiry_result_details()
                        .into_iter()
                        .map(|result| InquiryResponse {
                            address: result.peer_address.address_bytes().to_vec(),
                            page_scan_repetition_mode: 0,
                            class_of_device: result.class_of_device,
                            clock_offset: 0,
                            rssi: result.rssi.map_or(0, i32::from),
                            data: Some(data_types::pack(&result.extended_inquiry_response)),
                        })
                        .collect::<Vec<_>>();
                    let statuses = state.device.take_classic_inquiry_complete();
                    (responses, statuses)
                };
                for response in responses {
                    if sender.blocking_send(Ok(response)).is_err() {
                        break;
                    }
                }
                complete = !statuses.is_empty();
            }
            if !complete {
                if let Ok(mut state) = state.lock() {
                    let _ = successful_command(
                        &mut state.host,
                        Command::InquiryCancel,
                        "canceling inquiry",
                    );
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    async fn set_discoverability_mode(
        &self,
        request: Request<SetDiscoverabilityModeRequest>,
    ) -> Result<Response<()>, Status> {
        let discoverable = request.into_inner().mode != DiscoverabilityMode::NotDiscoverable as i32;
        self.runtime
            .blocking(move |state| {
                state.classic_discoverable = discoverable;
                state.apply_classic_scan_enable().map_err(internal)
            })
            .await?;
        Ok(Response::new(()))
    }

    async fn set_connectability_mode(
        &self,
        request: Request<SetConnectabilityModeRequest>,
    ) -> Result<Response<()>, Status> {
        let connectable = request.into_inner().mode != ConnectabilityMode::NotConnectable as i32;
        self.runtime
            .blocking(move |state| {
                state.classic_connectable = connectable;
                state.apply_classic_scan_enable().map_err(internal)
            })
            .await?;
        Ok(Response::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_unit_and_address_mapping_match_pandora() {
        assert_eq!(scan_interval(10.0), 16);
        assert_eq!(scan_interval(0.0), 0x10);
        let public = Address::from_bytes([1, 2, 3, 4, 5, 6], AddressType::PUBLIC_DEVICE);
        assert_eq!(
            address_variant(&public),
            scanning_response::Address::Public(vec![1, 2, 3, 4, 5, 6])
        );
    }

    #[test]
    fn advertising_report_flags_map_to_pandora_fields() {
        let response = legacy_response(AdvertisingReport {
            event_type: 0,
            address_type: 0,
            address: Address::from_bytes([1; 6], AddressType::PUBLIC_DEVICE),
            data: vec![2, 1, 6],
            rssi: -42,
        });
        assert!(response.legacy && response.connectable && response.scannable);
        assert_eq!(response.rssi, -42);
        assert!(matches!(
            response.address,
            Some(scanning_response::Address::Public(_))
        ));
    }
}
