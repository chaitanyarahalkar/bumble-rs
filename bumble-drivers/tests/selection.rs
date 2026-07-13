use bumble_drivers::{
    get_driver_for_host, CommandResponse, Driver, DriverHost, Error, FirmwareProvider, HciMetadata,
    Result,
};
use bumble_hci::Command;
use std::collections::BTreeMap;

struct EmptyFirmware;

impl FirmwareProvider for EmptyFirmware {
    fn load(&self, _file_name: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

struct Host {
    metadata: HciMetadata,
    command_count: usize,
}

impl DriverHost for Host {
    fn metadata(&self) -> &HciMetadata {
        &self.metadata
    }

    fn transact(&mut self, _command: Command) -> Result<CommandResponse> {
        self.command_count += 1;
        Err(Error::Host("unexpected probe".into()))
    }

    fn send_without_response(&mut self, _command: Command) -> Result<()> {
        Err(Error::Host("unexpected command".into()))
    }

    fn wait_vendor_event(&mut self, _event_type: u8) -> Result<Vec<u8>> {
        Err(Error::Host("unexpected event wait".into()))
    }
}

#[test]
fn explicit_intel_options_select_only_intel_without_probe() {
    let mut host = Host {
        metadata: BTreeMap::from([("driver".into(), "intel/ddc_addon:01AA".into())]),
        command_count: 0,
    };
    assert!(matches!(
        get_driver_for_host(&mut host, &EmptyFirmware).unwrap(),
        Some(Driver::Intel(_))
    ));
    assert_eq!(host.command_count, 0);
}

#[test]
fn unknown_explicit_driver_disables_auto_probe() {
    let mut host = Host {
        metadata: BTreeMap::from([
            ("driver".into(), "custom/options".into()),
            ("vendor_id".into(), "8087".into()),
            ("product_id".into(), "0032".into()),
        ]),
        command_count: 0,
    };
    assert!(get_driver_for_host(&mut host, &EmptyFirmware)
        .unwrap()
        .is_none());
    assert_eq!(host.command_count, 0);
}

#[test]
fn unforced_intel_usb_metadata_falls_through_rtk_then_selects_intel() {
    let mut host = Host {
        metadata: BTreeMap::from([
            ("vendor_id".into(), "8087".into()),
            ("product_id".into(), "0033".into()),
        ]),
        command_count: 0,
    };
    assert!(matches!(
        get_driver_for_host(&mut host, &EmptyFirmware).unwrap(),
        Some(Driver::Intel(_))
    ));
    assert_eq!(host.command_count, 0);
}
