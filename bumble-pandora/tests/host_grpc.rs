use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::HciPacket;
use bumble_pandora::proto::host_client::HostClient;
use bumble_pandora::proto::host_server::HostServer;
use bumble_pandora::proto::{
    ConnectabilityMode, DiscoverabilityMode, GetConnectionParametersRequest,
    SetConnectabilityModeRequest, SetDiscoverabilityModeRequest,
};
use bumble_pandora::{HostService, PandoraConfig, PandoraRuntime};
use bumble_transport::{PacketSink, PacketSource, TcpServer};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Endpoint, Server};
use tonic::Code;

fn drive_controller(server: TcpServer) {
    let transport = server.accept().expect("accept Pandora HCI transport");
    let (mut source, mut sink) = transport.try_split().expect("split HCI transport");
    let mut link = LocalLink::new();
    link.add_controller(Controller::new(
        "pandora-test-controller",
        Address::from_bytes([0xF0; 6], AddressType::PUBLIC_DEVICE),
    ));

    loop {
        let Some(packet) = source.read_packet().expect("read host HCI packet") else {
            return;
        };
        match packet {
            HciPacket::Command(command) => link.handle_command(0, command),
            HciPacket::AclData(packet) => {
                let _ = link.send_acl_packet(0, packet);
            }
            HciPacket::SyncData(packet) => {
                let _ = link.send_synchronous_data(
                    0,
                    packet.connection_handle,
                    packet.packet_status,
                    &packet.data,
                );
            }
            HciPacket::IsoData(packet) => {
                let _ = link.send_iso_packet(0, packet);
            }
            HciPacket::Event(_) | HciPacket::Custom(_) => {}
        }
        link.propagate_advertising();
        link.establish_connections();
        link.pump_ll();
        link.pump_periodic_sync_transfers();
        link.pump_classic();
        for event in link.drain_host_events(0) {
            sink.write_packet(&event)
                .expect("write controller HCI event");
        }
        sink.flush().expect("flush controller HCI events");
    }
}

async fn connect(endpoint: String) -> HostClient<Channel> {
    let channel = Endpoint::from_shared(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("connect Pandora gRPC client");
    HostClient::new(channel)
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_canonical_host_api_over_grpc_with_a_live_controller() {
    let controller_server = TcpServer::bind("127.0.0.1:0").expect("bind HCI server");
    let controller_address = controller_server.local_addr().expect("HCI server address");
    std::thread::spawn(move || drive_controller(controller_server));

    let runtime = PandoraRuntime::open(
        PandoraConfig::default(),
        0,
        Some(&format!("tcp-client:{controller_address}")),
    )
    .expect("open Pandora runtime");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gRPC server");
    let grpc_address = listener.local_addr().expect("gRPC server address");
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server = tokio::spawn(
        Server::builder()
            .add_service(HostServer::new(HostService::new(runtime)))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_receiver.await;
            }),
    );
    let mut client = connect(format!("http://{grpc_address}")).await;

    assert_eq!(
        client
            .read_local_address(())
            .await
            .expect("read local address")
            .into_inner()
            .address,
        vec![0xF0; 6]
    );
    client
        .set_discoverability_mode(SetDiscoverabilityModeRequest {
            mode: DiscoverabilityMode::DiscoverableGeneral as i32,
        })
        .await
        .expect("set discoverability");
    client
        .set_connectability_mode(SetConnectabilityModeRequest {
            mode: ConnectabilityMode::Connectable as i32,
        })
        .await
        .expect("set connectability");
    client.reset(()).await.expect("reset controller");
    client
        .factory_reset(())
        .await
        .expect("factory reset runtime");
    assert_eq!(
        client
            .get_connection_parameters(GetConnectionParametersRequest { connection: None })
            .await
            .expect_err("upstream leaves connection parameters unimplemented")
            .code(),
        Code::Unimplemented
    );

    drop(client);
    shutdown_sender.send(()).expect("request gRPC shutdown");
    server
        .await
        .expect("join gRPC server")
        .expect("stop gRPC server");
}
