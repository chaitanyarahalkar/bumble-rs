use bumble_drivers::rtk::{
    check, download_fragments, find_driver_info, parse_local_version, project_rom, Driver,
    Firmware, InitOutcome, HCI_RTK_DOWNLOAD_COMMAND, HCI_RTK_READ_ROM_VERSION_COMMAND,
    RTK_EXTENSION_SIGNATURE, RTK_FRAGMENT_LENGTH, RTK_ROM_LMP_8723B,
};
use bumble_drivers::{CommandResponse, DriverHost, Error, FirmwareProvider, HciMetadata, Result};
use bumble_hci::{Command, HciPacket, HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

fn response(bytes: &[u8]) -> CommandResponse {
    CommandResponse {
        num_hci_command_packets: 1,
        return_parameters: bytes.to_vec(),
    }
}

fn local_version(hci_version: u8, hci_subversion: u16, lmp_subversion: u16) -> CommandResponse {
    let mut bytes = vec![0, hci_version];
    bytes.extend_from_slice(&hci_subversion.to_le_bytes());
    bytes.push(hci_version);
    bytes.extend_from_slice(&0x005D_u16.to_le_bytes());
    bytes.extend_from_slice(&lmp_subversion.to_le_bytes());
    response(&bytes)
}

fn epatch(project_id: u8, version: u32, patches: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let count = patches.len();
    let table_end = 14 + count * 8;
    let mut offsets = Vec::with_capacity(count);
    let mut next_offset = table_end;
    for (_, patch) in patches {
        offsets.push(next_offset as u32);
        next_offset += patch.len();
    }

    let mut bytes = b"Realtech".to_vec();
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&(count as u16).to_le_bytes());
    for (chip_id, _) in patches {
        bytes.extend_from_slice(&chip_id.to_le_bytes());
    }
    for (_, patch) in patches {
        bytes.extend_from_slice(&(patch.len() as u16).to_le_bytes());
    }
    for offset in offsets {
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    for (_, patch) in patches {
        bytes.extend_from_slice(patch);
    }
    bytes.extend_from_slice(&[project_id, 1, 0]);
    bytes.extend_from_slice(&RTK_EXTENSION_SIGNATURE);
    bytes
}

fn patch(seed: u8, svn: u32) -> Vec<u8> {
    let mut patch = vec![seed, seed + 1, seed + 2, seed + 3];
    patch.extend_from_slice(&svn.to_le_bytes());
    patch.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
    patch
}

#[derive(Default)]
struct MapFirmware(BTreeMap<String, Vec<u8>>);

impl FirmwareProvider for MapFirmware {
    fn load(&self, file_name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.get(file_name).cloned())
    }
}

struct ScriptedHost {
    metadata: HciMetadata,
    responses: VecDeque<CommandResponse>,
    commands: Vec<Command>,
    no_response: Vec<Command>,
    delays: Vec<Duration>,
    timeout_first_reset: bool,
}

impl ScriptedHost {
    fn new(metadata: HciMetadata, responses: Vec<CommandResponse>) -> Self {
        Self {
            metadata,
            responses: responses.into(),
            commands: Vec::new(),
            no_response: Vec::new(),
            delays: Vec::new(),
            timeout_first_reset: false,
        }
    }
}

impl DriverHost for ScriptedHost {
    fn metadata(&self) -> &HciMetadata {
        &self.metadata
    }

    fn transact(&mut self, command: Command) -> Result<CommandResponse> {
        self.commands.push(command);
        self.responses
            .pop_front()
            .ok_or_else(|| Error::Host("script has no command response".into()))
    }

    fn transact_with_timeout(
        &mut self,
        command: Command,
        _timeout: Duration,
    ) -> Result<CommandResponse> {
        if self.timeout_first_reset {
            self.timeout_first_reset = false;
            self.commands.push(command);
            return Err(Error::Timeout("scripted reset timeout".into()));
        }
        self.transact(command)
    }

    fn send_without_response(&mut self, command: Command) -> Result<()> {
        self.no_response.push(command);
        Ok(())
    }

    fn wait_vendor_event(&mut self, _event_type: u8) -> Result<Vec<u8>> {
        Err(Error::Host(
            "Realtek driver does not wait for vendor events".into(),
        ))
    }

    fn delay(&mut self, duration: Duration) {
        self.delays.push(duration);
    }
}

#[test]
fn rtk_detection_project_map_and_driver_matrix_match_upstream() {
    assert!(check(&BTreeMap::from([
        ("vendor_id".into(), "0bda".into()),
        ("product_id".into(), "8771".into()),
    ])));
    assert!(check(&BTreeMap::from([("driver".into(), "rtk".into())])));
    assert!(!check(&BTreeMap::from([
        ("vendor_id".into(), "8087".into()),
        ("product_id".into(), "0032".into()),
    ])));
    assert_eq!(project_rom(1), Some(RTK_ROM_LMP_8723B));
    assert_eq!(project_rom(51), Some(0x8761));
    assert_eq!(project_rom(99), None);
    assert_eq!(
        find_driver_info(0x06, 0x0B, 0x8723).unwrap().firmware_name,
        "rtl8723b_fw.bin"
    );
    assert_eq!(
        find_driver_info(0x22, 0x0E, 0x8761).unwrap().firmware_name,
        "rtl8761cu_fw.bin",
        "version zero is the upstream wildcard"
    );
    assert!(find_driver_info(0x06, 0xFF, 0x8723).is_none());
}

#[test]
fn rtk_epatch_parser_selects_patch_and_replaces_tail_version() {
    let bytes = epatch(1, 0x1122_3344, &[(1, patch(0x10, 7)), (2, patch(0x20, 8))]);
    let firmware = Firmware::parse(&bytes).unwrap();
    assert_eq!(firmware.project_id, 1);
    assert_eq!(firmware.version, 0x1122_3344);
    assert_eq!(firmware.patches.len(), 2);
    assert_eq!(firmware.patches[0].svn_version, 7);
    assert_eq!(
        firmware.patch_for_rom_version(1).unwrap().payload,
        [0x20, 0x21, 0x22, 0x23, 8, 0, 0, 0, 0x44, 0x33, 0x22, 0x11]
    );

    assert!(Firmware::parse(b"not firmware").is_err());
    let mut no_project = bytes.clone();
    let extension = no_project.len() - RTK_EXTENSION_SIGNATURE.len();
    no_project[extension - 2] = 0;
    assert!(matches!(
        Firmware::parse(&no_project),
        Err(Error::InvalidFirmware(message)) if message.contains("zero-length")
    ));
    let mut bad_offset = bytes;
    bad_offset[22..26].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Firmware::parse(&bad_offset).is_err());
}

#[test]
fn rtk_download_fragment_indices_and_wire_match_upstream() {
    let payload = vec![0xA5; RTK_FRAGMENT_LENGTH * 2 + 1];
    let fragments = download_fragments(&payload);
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| (fragment.index, fragment.payload.len()))
            .collect::<Vec<_>>(),
        vec![(0, 252), (1, 252), (0x82, 1)]
    );
    let wire = HciPacket::Command(fragments[2].command()).to_bytes();
    assert_eq!(wire, [0x01, 0x20, 0xFC, 2, 0x82, 0xA5]);

    let wrapped = download_fragments(&vec![0; RTK_FRAGMENT_LENGTH * 130]);
    assert_eq!(wrapped[127].index, 0x7F);
    assert_eq!(wrapped[128].index, 0x00);
    assert_eq!(wrapped[129].index, 0x81);
}

#[test]
fn rtk_local_version_parser_pins_field_order_and_shape() {
    let local_response = local_version(0x06, 0x000B, 0x8723);
    let version = parse_local_version(&local_response).unwrap();
    assert_eq!(version.hci_version, 0x06);
    assert_eq!(version.hci_subversion, 0x000B);
    assert_eq!(version.lmp_version, 0x06);
    assert_eq!(version.company_identifier, 0x005D);
    assert_eq!(version.lmp_subversion, 0x8723);
    assert!(parse_local_version(&response(&[0])).is_err());
}

#[test]
fn rtk_driver_probes_downloads_selected_patch_appends_config_and_resets() {
    let firmware_version = 0x1122_3344;
    let firmware = epatch(1, firmware_version, &[(1, patch(0x10, 7))]);
    let config = vec![0x55, 0xAB, 0x23, 0x87];
    let provider = MapFirmware(BTreeMap::from([
        ("rtl8723b_fw.bin".into(), firmware),
        ("rtl8723b_config.bin".into(), config.clone()),
    ]));
    let mut host = ScriptedHost::new(
        BTreeMap::from([
            ("vendor_id".into(), "0bda".into()),
            ("product_id".into(), "8771".into()),
        ]),
        vec![
            response(&[0]),
            local_version(0x06, 0x000B, 0x8723),
            response(&[0, 0]),
            response(&[0, 0x80]),
            response(&[0, 1]),
            response(&[0]),
        ],
    );
    let driver = Driver::for_host(&mut host, &provider, false)
        .unwrap()
        .unwrap();
    assert_eq!(
        driver.init_controller(&mut host).unwrap(),
        InitOutcome {
            firmware_name: "rtl8723b_fw.bin",
            firmware_version: Some(firmware_version),
        }
    );
    assert_eq!(host.commands[0], Command::Reset);
    assert_eq!(
        host.commands[1].op_code(),
        HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND
    );
    assert_eq!(host.commands[2].op_code(), HCI_RTK_READ_ROM_VERSION_COMMAND);
    assert_eq!(host.commands[3].op_code(), HCI_RTK_DOWNLOAD_COMMAND);
    let download_wire = HciPacket::Command(host.commands[3].clone()).to_bytes();
    assert_eq!(download_wire[3], 17); // index + 12-byte patch + 4-byte config
    assert_eq!(download_wire[4], 0x80);
    assert_eq!(&download_wire[download_wire.len() - 4..], config);
    assert_eq!(host.commands.last(), Some(&Command::Reset));
    assert!(host.responses.is_empty());
}

#[test]
fn rtk_probe_retries_timeout_and_requires_mandatory_config() {
    let provider = MapFirmware(BTreeMap::from([("rtl8723d_fw.bin".into(), vec![1, 2, 3])]));
    let mut host = ScriptedHost::new(
        BTreeMap::from([("driver".into(), "rtk".into())]),
        vec![response(&[0]), local_version(0x08, 0x000D, 0x8723)],
    );
    host.timeout_first_reset = true;
    assert!(Driver::for_host(&mut host, &provider, false)
        .unwrap()
        .is_none());
    assert_eq!(host.commands[..2], [Command::Reset, Command::Reset]);
    assert_eq!(
        host.commands[2].op_code(),
        HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND
    );
}
