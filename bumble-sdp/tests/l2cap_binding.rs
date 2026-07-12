use bumble::Uuid;
use bumble_l2cap::{ChannelManager, ClassicChannelSpec};
use bumble_sdp::l2cap::{L2capSdpTransport, SdpL2capServer};
use bumble_sdp::service::{AttributeId, SdpClient, TransportError};
use bumble_sdp::{DataElement, ServiceAttribute, SDP_PSM};
use std::cell::Cell;
use std::rc::Rc;

fn relay(from: &mut ChannelManager, to: &mut ChannelManager) -> Result<usize, TransportError> {
    let mut count = 0;
    while let Some(pdu) = from.poll_outbound() {
        to.process_pdu(pdu)
            .map_err(|error| TransportError(format!("L2CAP relay failed: {error}")))?;
        count += 1;
    }
    Ok(count)
}

#[test]
fn sdp_client_continuation_runs_over_classic_l2cap() {
    let mut client_manager = ChannelManager::new();
    let mut server_manager = ChannelManager::new();
    server_manager
        .register_server(Some(SDP_PSM.into()), ClassicChannelSpec { mtu: 64 })
        .unwrap();
    // The server must split responses to fit the client's small receive MTU.
    let client_cid = client_manager
        .connect(SDP_PSM.into(), ClassicChannelSpec { mtu: 48 })
        .unwrap();
    for _ in 0..32 {
        let count = relay(&mut client_manager, &mut server_manager).unwrap()
            + relay(&mut server_manager, &mut client_manager).unwrap();
        if count == 0 {
            break;
        }
    }
    let server_cid = server_manager.poll_accepted_channel().unwrap();
    let mut endpoint = SdpL2capServer::new(server_cid, &server_manager).unwrap();
    endpoint.server_mut().add_service(
        0x0001_0000,
        vec![
            ServiceAttribute::new(0x0000, DataElement::unsigned_integer_32(0x0001_0000)),
            ServiceAttribute::new(
                0x0001,
                DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(0x1101))]),
            ),
            ServiceAttribute::new(
                0x0100,
                DataElement::text_string(b"Serial Port over negotiated L2CAP".to_vec()),
            ),
        ],
    );

    let request_count = Rc::new(Cell::new(0usize));
    let counted_requests = request_count.clone();
    let drive = move |client: &mut ChannelManager| -> Result<(), TransportError> {
        for _ in 0..16 {
            let mut count = relay(client, &mut server_manager)?;
            let served = endpoint.poll(&mut server_manager)?;
            counted_requests.set(counted_requests.get() + served);
            count += served;
            count += relay(&mut server_manager, client)?;
            if count == 0 {
                return Ok(());
            }
        }
        Err(TransportError("SDP/L2CAP stack did not quiesce".into()))
    };
    let transport = L2capSdpTransport::new(&mut client_manager, client_cid, drive).unwrap();
    let mut client = SdpClient::new(transport);

    assert_eq!(
        client
            .search_services(&[Uuid::from_16_bits(0x1101)])
            .unwrap(),
        vec![0x0001_0000]
    );
    let requests_before_attributes = request_count.get();
    let records = client
        .service_search_attribute(
            &[Uuid::from_16_bits(0x1101)],
            &[AttributeId::Range(0x0000, 0xffff)],
        )
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].len(), 3);
    assert!(
        request_count.get() - requests_before_attributes >= 2,
        "the small negotiated MTU must force an SDP continuation round-trip"
    );
    assert_eq!(
        ServiceAttribute::find(&records[0], 0x0100),
        Some(&DataElement::text_string(
            b"Serial Port over negotiated L2CAP".to_vec()
        ))
    );
}
