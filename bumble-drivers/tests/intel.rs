use bumble_drivers::intel::{
    check, ddc_records, device_info_from_tlv, firmware_base_name, reset_command,
    write_device_config_command, Driver, FirmwarePlan, FirmwareSearch, InitOutcome,
    ModeOfOperation, SecureBootEngineType, Value, ValueType, HCI_INTEL_READ_VERSION_COMMAND,
    HCI_INTEL_RESET_COMMAND, HCI_INTEL_SECURE_SEND_COMMAND, HCI_INTEL_WRITE_DEVICE_CONFIG_COMMAND,
    STANDARD_RESET_OPCODE,
};
use bumble_drivers::{CommandResponse, DriverHost, Error, FirmwareProvider, HciMetadata, Result};
use bumble_hci::{Command, HciPacket};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn intel_tlv(mode: ModeOfOperation) -> Vec<u8> {
    vec![
        0x10, 4, 0x00, 0x04, 0x00, 0x00, // CNVI -> 0040
        0x11, 4, 0x10, 0x04, 0x00, 0x00, // CNVR -> 0041
        0x12, 4, 0x00, 0x37, 0x17, 0x00, // Intel 37, Typhoon Peak
        0x1C, 1, mode as u8, 0x2F, 1, 0x00, // RSA
        0x30, 6, 1, 2, 3, 4, 5, 6, 0x91, 2, 0xAA, 0xBB, // unknown TLV is preserved
        0x00, 0x00,
    ]
}

fn sfi_image() -> Vec<u8> {
    let mut image = vec![0xA5; 964];
    image.extend_from_slice(&[
        0x0E, 0xFC, 4, 0x78, 0x56, 0x34, 0x12, // boot params: seven bytes
        0x34, 0x12, 2, 0xAA, 0xBB, // second command makes a 12-byte group
    ]);
    image
}

fn response(status: u8) -> CommandResponse {
    CommandResponse {
        num_hci_command_packets: 1,
        return_parameters: vec![status],
    }
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
    vendor_events: VecDeque<(u8, Vec<u8>)>,
}

impl ScriptedHost {
    fn new(metadata: HciMetadata, responses: Vec<CommandResponse>) -> Self {
        Self {
            metadata,
            responses: responses.into(),
            commands: Vec::new(),
            no_response: Vec::new(),
            vendor_events: VecDeque::from([(0x06, vec![0x06]), (0x02, vec![0x02])]),
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

    fn send_without_response(&mut self, command: Command) -> Result<()> {
        self.no_response.push(command);
        Ok(())
    }

    fn wait_vendor_event(&mut self, event_type: u8) -> Result<Vec<u8>> {
        let (actual, event) = self
            .vendor_events
            .pop_front()
            .ok_or_else(|| Error::Host("script has no vendor event".into()))?;
        if actual != event_type {
            return Err(Error::Host(format!(
                "expected vendor event {event_type}, got {actual}"
            )));
        }
        Ok(event)
    }
}

#[test]
fn intel_detection_options_and_firmware_search_match_upstream_order() {
    assert!(check(&BTreeMap::from([
        ("vendor_id".into(), "8087".into()),
        ("product_id".into(), "0032".into()),
    ])));
    assert!(check(&BTreeMap::from([(
        "driver".into(),
        "intel/ddc_addon:02AABB+ddc_override:01CC".into(),
    )])));
    assert!(!check(&BTreeMap::from([
        ("vendor_id".into(), "8087".into()),
        ("product_id".into(), "FFFF".into()),
    ])));

    let host = ScriptedHost::new(
        BTreeMap::from([(
            "driver".into(),
            "intel/ddc_addon:02AABB+ddc_override:01CC".into(),
        )]),
        Vec::new(),
    );
    let driver = Driver::for_host(&host, false).unwrap().unwrap();
    assert_eq!(driver.options().ddc_addon, Some(vec![2, 0xAA, 0xBB]));
    assert_eq!(driver.options().ddc_override, Some(vec![1, 0xCC]));

    let root = temporary_directory();
    let env = root.join("env");
    let project = root.join("project");
    fs::create_dir_all(&env).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("test.sfi"), b"project").unwrap();
    let search = FirmwareSearch {
        environment_directory: Some(env.clone()),
        project_directory: Some(project.clone()),
        ..Default::default()
    };
    assert_eq!(
        search.find("test.sfi"),
        None,
        "env override blocks fallback"
    );
    fs::write(env.join("test.sfi"), b"environment").unwrap();
    assert_eq!(search.find("test.sfi"), Some(env.join("test.sfi")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn intel_tlv_decodes_typed_and_open_values() {
    let info = device_info_from_tlv(&intel_tlv(ModeOfOperation::Bootloader)).unwrap();
    assert_eq!(firmware_base_name(&info).unwrap(), "ibt-0040-0041");
    assert_eq!(
        info[&ValueType::CURRENT_MODE_OF_OPERATION],
        Value::U8(ModeOfOperation::Bootloader as u8)
    );
    assert_eq!(
        info[&ValueType::BLUETOOTH_ADDRESS],
        Value::BluetoothAddress([1, 2, 3, 4, 5, 6])
    );
    assert_eq!(info[&ValueType(0x91)], Value::Bytes(vec![0xAA, 0xBB]));

    assert!(matches!(
        device_info_from_tlv(&[0x10, 4, 1, 2]),
        Err(Error::InvalidResponse(_))
    ));
    assert!(matches!(
        device_info_from_tlv(&[0x1C, 2, 1, 2]),
        Err(Error::InvalidResponse(_))
    ));
}

#[test]
fn intel_sfi_plan_preserves_sections_alignment_and_boot_address() {
    let plan = FirmwarePlan::parse(&sfi_image(), SecureBootEngineType::Rsa).unwrap();
    assert_eq!(plan.boot_address, 0x1234_5678);
    assert_eq!(
        plan.secure_sends
            .iter()
            .map(|send| (send.data_type, send.data.len()))
            .collect::<Vec<_>>(),
        vec![(0, 128), (3, 252), (3, 4), (2, 252), (2, 4), (1, 12)]
    );
    let wire = HciPacket::Command(plan.secure_sends[0].command()).to_bytes();
    assert_eq!(&wire[..5], &[0x01, 0x09, 0xFC, 129, 0x00]);

    let ecdsa = FirmwarePlan::parse(&sfi_image(), SecureBootEngineType::Ecdsa).unwrap();
    assert_eq!(ecdsa.boot_address, 0x1234_5678);
    assert_eq!(
        ecdsa
            .secure_sends
            .iter()
            .map(|send| (send.data_type, send.data.len()))
            .collect::<Vec<_>>(),
        vec![(0, 128), (3, 96), (2, 96), (1, 12)]
    );

    assert!(matches!(
        FirmwarePlan::parse(&vec![0; 963], SecureBootEngineType::Rsa),
        Err(Error::InvalidFirmware(_))
    ));
    let mut truncated = vec![0; 964];
    truncated.extend_from_slice(&[0x0E, 0xFC, 4, 1]);
    assert!(matches!(
        FirmwarePlan::parse(&truncated, SecureBootEngineType::Rsa),
        Err(Error::InvalidFirmware(_))
    ));
}

#[test]
fn intel_ddc_and_reset_commands_match_vendor_wire_format() {
    assert_eq!(
        ddc_records(&[2, 0xAA, 0xBB, 1, 0xCC]).unwrap(),
        vec![vec![2, 0xAA, 0xBB], vec![1, 0xCC]]
    );
    assert!(ddc_records(&[3, 1, 2]).is_err());
    assert_eq!(
        HciPacket::Command(write_device_config_command(vec![2, 0xAA, 0xBB])).to_bytes(),
        [0x01, 0x8B, 0xFC, 3, 2, 0xAA, 0xBB]
    );
    assert_eq!(
        HciPacket::Command(reset_command(0, 1, 0, 1, 0x1234_5678)).to_bytes(),
        [0x01, 0x01, 0xFC, 8, 0, 1, 0, 1, 0x78, 0x56, 0x34, 0x12]
    );
}

#[test]
fn intel_driver_loads_firmware_waits_events_resets_and_applies_ddc() {
    let mut version = vec![0];
    version.extend_from_slice(&intel_tlv(ModeOfOperation::Bootloader));
    let mut responses = vec![CommandResponse {
        num_hci_command_packets: 1,
        return_parameters: vec![1], // bootloader reset response is unknown-command
    }];
    responses.push(CommandResponse {
        num_hci_command_packets: 1,
        return_parameters: version,
    });
    responses.extend((0..8).map(|_| response(0))); // six SFI + two DDC records

    let mut host = ScriptedHost::new(
        BTreeMap::from([
            ("vendor_id".into(), "8087".into()),
            ("product_id".into(), "0032".into()),
        ]),
        responses,
    );
    let firmware = MapFirmware(BTreeMap::from([
        ("ibt-0040-0041.sfi".into(), sfi_image()),
        ("ibt-0040-0041.ddc".into(), vec![2, 0xAA, 0xBB, 1, 0xCC]),
    ]));
    let driver = Driver::for_host(&host, false).unwrap().unwrap();
    assert_eq!(
        driver.init_controller(&mut host, &firmware).unwrap(),
        InitOutcome::FirmwareLoaded {
            firmware_name: "ibt-0040-0041.sfi".into(),
            boot_address: 0x1234_5678,
            secure_send_count: 6,
        }
    );
    assert_eq!(host.commands[0].op_code(), STANDARD_RESET_OPCODE);
    assert_eq!(host.commands[1].op_code(), HCI_INTEL_READ_VERSION_COMMAND);
    assert_eq!(
        host.commands
            .iter()
            .filter(|command| command.op_code() == HCI_INTEL_SECURE_SEND_COMMAND)
            .count(),
        6
    );
    assert_eq!(
        host.commands
            .iter()
            .filter(|command| command.op_code() == HCI_INTEL_WRITE_DEVICE_CONFIG_COMMAND)
            .count(),
        2
    );
    assert_eq!(host.no_response.len(), 1);
    assert_eq!(host.no_response[0].op_code(), HCI_INTEL_RESET_COMMAND);
    assert!(host.responses.is_empty());
    assert!(host.vendor_events.is_empty());
}

#[test]
fn operational_intel_controller_applies_override_then_addon_without_sfi() {
    let mut version = vec![0];
    version.extend_from_slice(&intel_tlv(ModeOfOperation::Operational));
    let mut host = ScriptedHost::new(
        BTreeMap::from([(
            "driver".into(),
            "intel/ddc_override:01AA+ddc_addon:02BBCC".into(),
        )]),
        vec![
            response(0),
            CommandResponse {
                num_hci_command_packets: 1,
                return_parameters: version,
            },
            response(0),
            response(0),
        ],
    );
    let driver = Driver::for_host(&host, false).unwrap().unwrap();
    assert_eq!(
        driver
            .init_controller(&mut host, &MapFirmware::default())
            .unwrap(),
        InitOutcome::AlreadyOperational
    );
    let ddc_wires = host.commands[2..]
        .iter()
        .cloned()
        .map(|command| HciPacket::Command(command).to_bytes())
        .collect::<Vec<_>>();
    assert_eq!(
        ddc_wires,
        vec![
            vec![0x01, 0x8B, 0xFC, 2, 1, 0xAA],
            vec![0x01, 0x8B, 0xFC, 3, 2, 0xBB, 0xCC],
        ]
    );
}

fn temporary_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "bumble-intel-driver-{}-{nonce}",
        std::process::id()
    ))
}
