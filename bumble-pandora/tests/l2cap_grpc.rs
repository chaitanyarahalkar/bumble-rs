use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::HciPacket;
use bumble_pandora::proto::connect_le_request;
use bumble_pandora::proto::connect_le_response;
use bumble_pandora::proto::host_client::HostClient;
use bumble_pandora::proto::host_server::HostServer;
use bumble_pandora::proto::l2cap::connect_request;
use bumble_pandora::proto::l2cap::connect_response;
use bumble_pandora::proto::l2cap::disconnect_response;
use bumble_pandora::proto::l2cap::l2cap_client::L2capClient;
use bumble_pandora::proto::l2cap::l2cap_server::L2capServer;
use bumble_pandora::proto::l2cap::send_request;
use bumble_pandora::proto::l2cap::send_response;
use bumble_pandora::proto::l2cap::wait_connection_request;
use bumble_pandora::proto::l2cap::wait_connection_response;
use bumble_pandora::proto::l2cap::wait_disconnection_response;
use bumble_pandora::proto::l2cap::{
    ConnectRequest as L2capConnectRequest, CreditBasedChannelRequest, DisconnectRequest,
    ReceiveRequest, SendRequest, WaitConnectionRequest as L2capWaitConnectionRequest,
    WaitDisconnectionRequest as L2capWaitDisconnectionRequest,
};
use bumble_pandora::proto::{AdvertiseRequest, ConnectLeRequest};
use bumble_pandora::{HostService, L2capService, PandoraConfig, PandoraRuntime, ServerConfig};
use bumble_transport::{PacketSink, PacketSource, TcpServer};
use std::sync::mpsc;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Endpoint, Server};

enum HostInput {
    Packet(usize, Box<HciPacket>),
    End,
}

fn drive_controllers(servers: [TcpServer; 2]) {
    let transports = servers.map(|server| server.accept().expect("accept Pandora HCI transport"));
    let mut link = LocalLink::new();
    let mut sinks: Vec<Box<dyn PacketSink + Send>> = Vec::new();
    let (input_sender, input_receiver) = mpsc::channel();
    for (controller_id, transport) in transports.into_iter().enumerate() {
        link.add_controller(Controller::new(
            &format!("pandora-l2cap-{controller_id}"),
            Address::from_bytes([0xF0 + controller_id as u8; 6], AddressType::PUBLIC_DEVICE),
        ));
        let (mut source, sink) = transport.try_split().expect("split HCI transport");
        sinks.push(Box::new(sink));
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
                Ok(None) | Err(_) => {
                    let _ = input_sender.send(HostInput::End);
                    return;
                }
            }
        });
    }
    drop(input_sender);

    while let Ok(input) = input_receiver.recv() {
        let HostInput::Packet(controller_id, packet) = input else {
            return;
        };
        match *packet {
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
        link.propagate_advertising();
        link.establish_connections();
        link.pump_ll();
        link.pump_periodic_sync_transfers();
        link.pump_classic();
        for (controller_id, sink) in sinks.iter_mut().enumerate() {
            for event in link.drain_host_events(controller_id) {
                sink.write_packet(&event)
                    .expect("write controller HCI event");
            }
            sink.flush().expect("flush controller HCI events");
        }
    }
}

fn config(address: &str) -> PandoraConfig {
    PandoraConfig {
        address: address.into(),
        server: ServerConfig::default(),
        ..PandoraConfig::default()
    }
}

async fn start_server(
    runtime: PandoraRuntime,
) -> (
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Pandora gRPC server");
    let address = listener.local_addr().expect("gRPC server address");
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server = tokio::spawn(
        Server::builder()
            .add_service(HostServer::new(HostService::new(runtime.clone())))
            .add_service(L2capServer::new(L2capService::new(runtime)))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_receiver.await;
            }),
    );
    (format!("http://{address}"), shutdown_sender, server)
}

async fn channel(endpoint: &str) -> Channel {
    Endpoint::from_shared(endpoint.to_owned())
        .expect("valid endpoint")
        .connect()
        .await
        .expect("connect Pandora client")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transfers_an_le_credit_sdu_through_two_live_grpc_servers() {
    let controller_servers = [
        TcpServer::bind("127.0.0.1:0").expect("bind first HCI server"),
        TcpServer::bind("127.0.0.1:0").expect("bind second HCI server"),
    ];
    let controller_addresses = controller_servers
        .iter()
        .map(|server| server.local_addr().expect("HCI server address"))
        .collect::<Vec<_>>();
    std::thread::spawn(move || drive_controllers(controller_servers));

    let first_address = controller_addresses[0];
    let second_address = controller_addresses[1];
    let first_runtime = std::thread::spawn(move || {
        PandoraRuntime::open(
            config("C4:F2:17:1A:1D:AA"),
            0,
            Some(&format!("tcp-client:{first_address}")),
        )
        .expect("open first Pandora runtime")
    });
    let second_runtime = std::thread::spawn(move || {
        PandoraRuntime::open(
            config("C4:F2:17:1A:1D:BB"),
            0,
            Some(&format!("tcp-client:{second_address}")),
        )
        .expect("open second Pandora runtime")
    });
    let central_runtime = first_runtime.join().expect("join first runtime opener");
    let peripheral_runtime = second_runtime.join().expect("join second runtime opener");
    let (central_endpoint, central_shutdown, central_server) = start_server(central_runtime).await;
    let (peripheral_endpoint, peripheral_shutdown, peripheral_server) =
        start_server(peripheral_runtime).await;

    let mut central_host = HostClient::new(channel(&central_endpoint).await);
    let mut peripheral_host = HostClient::new(channel(&peripheral_endpoint).await);
    let mut central_l2cap = L2capClient::new(channel(&central_endpoint).await);
    let mut peripheral_l2cap = L2capClient::new(channel(&peripheral_endpoint).await);

    let mut advertisements = peripheral_host
        .advertise(AdvertiseRequest {
            legacy: true,
            own_address_type: 1,
            connectable: true,
            ..Default::default()
        })
        .await
        .expect("start peripheral advertising")
        .into_inner();
    let peripheral_address = Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE)
        .expect("parse peripheral address");
    let central_connection = match central_host
        .connect_le(ConnectLeRequest {
            own_address_type: 1,
            address: Some(connect_le_request::Address::Random(
                peripheral_address.address_bytes().to_vec(),
            )),
        })
        .await
        .expect("connect LE")
        .into_inner()
        .result
        .expect("LE connection result")
    {
        connect_le_response::Result::Connection(connection) => connection,
        _ => panic!("expected an LE connection"),
    };
    let peripheral_connection = advertisements
        .message()
        .await
        .expect("receive advertising stream")
        .expect("peripheral connection")
        .connection
        .expect("peripheral connection cookie");
    drop(advertisements);

    let channel_request = CreditBasedChannelRequest {
        spsm: 0x80,
        mtu: 128,
        mps: 64,
        initial_credit: 8,
    };
    let wait_request = L2capWaitConnectionRequest {
        connection: Some(peripheral_connection),
        r#type: Some(wait_connection_request::Type::LeCreditBased(
            channel_request,
        )),
    };
    let wait = tokio::spawn(async move {
        peripheral_l2cap
            .wait_connection(wait_request)
            .await
            .expect("wait for incoming L2CAP channel")
            .into_inner()
    });
    let central_channel = match central_l2cap
        .connect(L2capConnectRequest {
            connection: Some(central_connection),
            r#type: Some(connect_request::Type::LeCreditBased(channel_request)),
        })
        .await
        .expect("connect L2CAP channel")
        .into_inner()
        .result
        .expect("L2CAP connect result")
    {
        connect_response::Result::Channel(channel) => channel,
        _ => panic!("expected a connected L2CAP channel"),
    };
    let peripheral_channel = match wait
        .await
        .expect("join L2CAP wait")
        .result
        .expect("incoming L2CAP result")
    {
        wait_connection_response::Result::Channel(channel) => channel,
        _ => panic!("expected an incoming L2CAP channel"),
    };

    let mut receiver = L2capClient::new(channel(&peripheral_endpoint).await)
        .receive(ReceiveRequest {
            source: Some(
                bumble_pandora::proto::l2cap::receive_request::Source::Channel(
                    peripheral_channel.clone(),
                ),
            ),
        })
        .await
        .expect("start L2CAP receive stream")
        .into_inner();
    let send = central_l2cap
        .send(SendRequest {
            sink: Some(send_request::Sink::Channel(central_channel.clone())),
            data: b"pandora-l2cap".to_vec(),
        })
        .await
        .expect("send L2CAP SDU")
        .into_inner();
    assert!(matches!(
        send.result,
        Some(send_response::Result::Success(()))
    ));
    assert_eq!(
        receiver
            .message()
            .await
            .expect("receive L2CAP stream item")
            .expect("received L2CAP SDU")
            .data,
        b"pandora-l2cap"
    );
    drop(receiver);

    let mut peripheral_wait_client = L2capClient::new(channel(&peripheral_endpoint).await);
    let wait_channel = peripheral_channel.clone();
    let disconnected = tokio::spawn(async move {
        peripheral_wait_client
            .wait_disconnection(L2capWaitDisconnectionRequest {
                channel: Some(wait_channel),
            })
            .await
            .expect("wait for L2CAP disconnection")
            .into_inner()
    });
    let disconnect = central_l2cap
        .disconnect(DisconnectRequest {
            channel: Some(central_channel),
        })
        .await
        .expect("disconnect L2CAP channel")
        .into_inner();
    assert!(matches!(
        disconnect.result,
        Some(disconnect_response::Result::Success(()))
    ));
    assert!(matches!(
        disconnected
            .await
            .expect("join L2CAP disconnection wait")
            .result,
        Some(wait_disconnection_response::Result::Success(()))
    ));

    drop(central_host);
    drop(peripheral_host);
    drop(central_l2cap);
    central_shutdown.send(()).expect("stop central gRPC server");
    peripheral_shutdown
        .send(())
        .expect("stop peripheral gRPC server");
    central_server
        .await
        .expect("join central gRPC server")
        .expect("central gRPC server result");
    peripheral_server
        .await
        .expect("join peripheral gRPC server")
        .expect("peripheral gRPC server result");
}
