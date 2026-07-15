use bumble_hci::{AclDataPacket, Command, HciPacket, IsoDataPacket};
use bumble_host::{
    CigParameters, CisParameters, Device, HostTransport, DEFAULT_ISO_CIS_MAX_SDU,
    DEFAULT_ISO_CIS_MAX_TRANSPORT_LATENCY, DEFAULT_ISO_CIS_RTN,
};

#[derive(Default)]
struct CaptureTransport {
    commands: Vec<Command>,
}

impl HostTransport for CaptureTransport {
    fn handle_command(&mut self, _controller_id: usize, command: Command) {
        self.commands.push(command);
    }

    fn send_acl_packet(&mut self, _controller_id: usize, _packet: AclDataPacket) -> bool {
        false
    }

    fn send_synchronous_data(
        &mut self,
        _controller_id: usize,
        _connection_handle: u16,
        _packet_status: u8,
        _data: &[u8],
    ) -> bool {
        false
    }

    fn send_iso_packet(&mut self, _controller_id: usize, _packet: IsoDataPacket) -> bool {
        false
    }

    fn drain_host_events(&mut self, _controller_id: usize) -> Vec<HciPacket> {
        Vec::new()
    }
}

#[test]
fn upstream_cis_defaults_and_unidirectional_rtn_normalization_are_exact() {
    let defaults = CisParameters::new(1);
    assert_eq!(defaults.max_sdu_c_to_p, DEFAULT_ISO_CIS_MAX_SDU);
    assert_eq!(defaults.max_sdu_p_to_c, DEFAULT_ISO_CIS_MAX_SDU);
    assert_eq!(defaults.phy_c_to_p, 0x02);
    assert_eq!(defaults.phy_p_to_c, 0x02);
    assert_eq!(defaults.rtn_c_to_p, DEFAULT_ISO_CIS_RTN);
    assert_eq!(defaults.rtn_p_to_c, DEFAULT_ISO_CIS_RTN);

    let mut c_to_p_only = CisParameters::new(2);
    c_to_p_only.max_sdu_p_to_c = 0;
    c_to_p_only.phy_p_to_c = 0x03;
    c_to_p_only.rtn_p_to_c = 0x7F;

    let mut p_to_c_only = CisParameters::new(3);
    p_to_c_only.max_sdu_c_to_p = 0;
    p_to_c_only.phy_c_to_p = 0x03;
    p_to_c_only.rtn_c_to_p = 0x7F;

    let mut parameters = CigParameters::new(4, vec![c_to_p_only, p_to_c_only], 7_500, 10_000);
    parameters.worst_case_sca = 7;
    parameters.packing = 1;
    parameters.framing = 1;
    parameters.max_transport_latency_c_to_p = 55;
    parameters.max_transport_latency_p_to_c = 66;

    let mut device = Device::new(0);
    let mut transport = CaptureTransport::default();
    assert!(device.configure_cig_with_parameters(&mut transport, &parameters));
    assert_eq!(transport.commands.len(), 1);
    assert_eq!(
        transport.commands.pop().unwrap(),
        Command::LeSetCigParameters {
            cig_id: 4,
            sdu_interval_c_to_p: 7_500,
            sdu_interval_p_to_c: 10_000,
            worst_case_sca: 7,
            packing: 1,
            framing: 1,
            max_transport_latency_c_to_p: 55,
            max_transport_latency_p_to_c: 66,
            cis_id: vec![2, 3],
            max_sdu_c_to_p: vec![DEFAULT_ISO_CIS_MAX_SDU, 0],
            max_sdu_p_to_c: vec![0, DEFAULT_ISO_CIS_MAX_SDU],
            phy_c_to_p: vec![0x02, 0x03],
            phy_p_to_c: vec![0x03, 0x02],
            rtn_c_to_p: vec![DEFAULT_ISO_CIS_RTN, 0],
            rtn_p_to_c: vec![0, DEFAULT_ISO_CIS_RTN],
        }
    );
    assert_eq!(
        CigParameters::new(1, vec![CisParameters::new(1)], 0, 0).max_transport_latency_c_to_p,
        DEFAULT_ISO_CIS_MAX_TRANSPORT_LATENCY
    );
}

#[test]
fn invalid_cig_shapes_are_rejected_before_hci_serialization() {
    let mut device = Device::new(0);
    let mut transport = CaptureTransport::default();
    assert!(!device.configure_cig_with_parameters(
        &mut transport,
        &CigParameters::new(1, Vec::new(), 10_000, 10_000),
    ));
    assert!(!device.configure_cig_with_parameters(
        &mut transport,
        &CigParameters::new(1, vec![CisParameters::new(1)], 0x0100_0000, 10_000,),
    ));
    assert!(transport.commands.is_empty());
}
