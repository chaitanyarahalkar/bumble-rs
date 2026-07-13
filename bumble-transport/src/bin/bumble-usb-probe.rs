use rusb::{Device, DeviceDescriptor, Direction, GlobalContext, TransferType};
use std::collections::BTreeMap;
use std::process::ExitCode;

const USB_DEVICE_CLASS_DEVICE: u8 = 0x00;
const USB_DEVICE_CLASS_WIRELESS_CONTROLLER: u8 = 0xE0;
const USB_DEVICE_SUBCLASS_RF_CONTROLLER: u8 = 0x01;
const USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER: u8 = 0x01;
const USB_BT_HCI_CLASS_TUPLE: (u8, u8, u8) = (
    USB_DEVICE_CLASS_WIRELESS_CONTROLLER,
    USB_DEVICE_SUBCLASS_RF_CONTROLLER,
    USB_DEVICE_PROTOCOL_BLUETOOTH_PRIMARY_CONTROLLER,
);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Args {
    verbose: bool,
    hci_only: bool,
    manufacturer: Option<String>,
    product: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EndpointInfo {
    address: u8,
    transfer_type: TransferType,
    direction: Direction,
    max_packet_size: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InterfaceInfo {
    number: u8,
    setting: u8,
    setting_count: usize,
    class: u8,
    subclass: u8,
    protocol: u8,
    endpoints: Vec<EndpointInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfigurationInfo {
    number: u8,
    interfaces: Vec<InterfaceInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceInfo {
    vendor_id: u16,
    product_id: u16,
    bus: u8,
    address: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    serial: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
    configurations: Vec<ConfigurationInfo>,
}

fn usage() -> &'static str {
    "usage: bumble-usb-probe [--verbose] [--hci-only] [--manufacturer NAME] [--product NAME]"
}

fn option_value(
    argument: &str,
    option: &str,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<Option<String>, String> {
    if argument == option {
        return arguments
            .next()
            .map(Some)
            .ok_or_else(|| format!("missing value for {option}"));
    }
    Ok(argument
        .strip_prefix(&format!("{option}="))
        .map(ToOwned::to_owned))
}

fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut args = Args::default();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage().into()),
            "--verbose" => {
                args.verbose = true;
                continue;
            }
            "--hci-only" => {
                args.hci_only = true;
                continue;
            }
            _ => {}
        }
        if let Some(value) = option_value(&argument, "--manufacturer", &mut arguments)? {
            args.manufacturer = Some(value);
            continue;
        }
        if let Some(value) = option_value(&argument, "--product", &mut arguments)? {
            args.product = Some(value);
            continue;
        }
        return Err(format!("unknown argument {argument:?}"));
    }
    Ok(args)
}

fn class_name(class: u8) -> String {
    match class {
        0x00 => "Device".into(),
        0x01 => "Audio".into(),
        0x02 => "Communications and CDC Control".into(),
        0x03 => "Human Interface Device".into(),
        0x05 => "Physical".into(),
        0x06 => "Still Imaging".into(),
        0x07 => "Printer".into(),
        0x08 => "Mass Storage".into(),
        0x09 => "Hub".into(),
        0x0A => "CDC Data".into(),
        0x0B => "Smart Card".into(),
        0x0D => "Content Security".into(),
        0x0E => "Video".into(),
        0x0F => "Personal Healthcare".into(),
        0x10 => "Audio/Video".into(),
        0x11 => "Billboard".into(),
        0x12 => "USB Type-C Bridge".into(),
        0x3C => "I3C".into(),
        0xDC => "Diagnostic".into(),
        0xE0 => "Wireless Controller".into(),
        0xEF => "Miscellaneous".into(),
        0xFE => "Application Specific".into(),
        0xFF => "Vendor Specific".into(),
        value => format!("0x{value:02X}"),
    }
}

fn class_info(class: u8, subclass: u8, protocol: u8) -> (String, String) {
    let protocol_name = match (class, subclass, protocol) {
        (0xE0, 0x01, 0x01) => Some("Bluetooth"),
        (0xE0, 0x01, 0x02) => Some("UWB"),
        (0xE0, 0x01, 0x03) => Some("Remote NDIS"),
        (0xE0, 0x01, 0x04) => Some("Bluetooth AMP"),
        _ => None,
    };
    let suffix = protocol_name
        .map(|name| format!(" [{name}]"))
        .unwrap_or_default();
    (class_name(class), format!("{subclass}/{protocol}{suffix}"))
}

fn is_bluetooth_hci(device: &DeviceInfo) -> bool {
    if (device.class, device.subclass, device.protocol) == USB_BT_HCI_CLASS_TUPLE {
        return true;
    }
    device.class == USB_DEVICE_CLASS_DEVICE
        && device.configurations.iter().any(|configuration| {
            configuration.interfaces.iter().any(|interface| {
                (interface.class, interface.subclass, interface.protocol) == USB_BT_HCI_CLASS_TUPLE
            })
        })
}

fn inspect_device(device: &Device<GlobalContext>, descriptor: &DeviceDescriptor) -> DeviceInfo {
    let mut configurations = Vec::new();
    for index in 0..descriptor.num_configurations() {
        let Ok(configuration) = device.config_descriptor(index) else {
            continue;
        };
        let mut interfaces = Vec::new();
        for interface in configuration.interfaces() {
            let setting_count = interface.descriptors().count();
            for setting in interface.descriptors() {
                let endpoints = setting
                    .endpoint_descriptors()
                    .map(|endpoint| EndpointInfo {
                        address: endpoint.address(),
                        transfer_type: endpoint.transfer_type(),
                        direction: endpoint.direction(),
                        max_packet_size: endpoint.max_packet_size(),
                    })
                    .collect();
                interfaces.push(InterfaceInfo {
                    number: setting.interface_number(),
                    setting: setting.setting_number(),
                    setting_count,
                    class: setting.class_code(),
                    subclass: setting.sub_class_code(),
                    protocol: setting.protocol_code(),
                    endpoints,
                });
            }
        }
        configurations.push(ConfigurationInfo {
            number: configuration.number(),
            interfaces,
        });
    }

    let (serial, manufacturer, product) = match device.open() {
        Ok(handle) => (
            handle.read_serial_number_string_ascii(descriptor).ok(),
            handle.read_manufacturer_string_ascii(descriptor).ok(),
            handle.read_product_string_ascii(descriptor).ok(),
        ),
        Err(_) => (None, None, None),
    };
    DeviceInfo {
        vendor_id: descriptor.vendor_id(),
        product_id: descriptor.product_id(),
        bus: device.bus_number(),
        address: device.address(),
        class: descriptor.class_code(),
        subclass: descriptor.sub_class_code(),
        protocol: descriptor.protocol_code(),
        serial,
        manufacturer,
        product,
        configurations,
    }
}

fn endpoint_type_name(transfer_type: TransferType) -> &'static str {
    match transfer_type {
        TransferType::Control => "CONTROL",
        TransferType::Isochronous => "ISOCHRONOUS",
        TransferType::Bulk => "BULK",
        TransferType::Interrupt => "INTERRUPT",
    }
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::In => "IN",
        Direction::Out => "OUT",
    }
}

fn render_device(device: &DeviceInfo, names: &[String], verbose: bool) -> String {
    let (class, subclass) = class_info(device.class, device.subclass, device.protocol);
    let mut lines = vec![format!(
        "ID {:04X}:{:04X}",
        device.vendor_id, device.product_id
    )];
    if !names.is_empty() {
        lines.push(format!("  Bumble Transport Names: {}", names.join(" or ")));
    }
    lines.push(format!(
        "  Bus/Device:             {:03}/{:03}",
        device.bus, device.address
    ));
    lines.push(format!("  Class:                  {class}"));
    lines.push(format!("  Subclass/Protocol:      {subclass}"));
    if let Some(serial) = &device.serial {
        lines.push(format!("  Serial:                 {serial}"));
    }
    if let Some(manufacturer) = &device.manufacturer {
        lines.push(format!("  Manufacturer:           {manufacturer}"));
    }
    if let Some(product) = &device.product {
        lines.push(format!("  Product:                {product}"));
    }
    if verbose {
        for configuration in &device.configurations {
            lines.push(format!("  Configuration {}", configuration.number));
            for interface in &configuration.interfaces {
                let alternate = if interface.setting_count > 1 {
                    format!("/{}", interface.setting)
                } else {
                    String::new()
                };
                let (class, subclass) =
                    class_info(interface.class, interface.subclass, interface.protocol);
                lines.push(format!(
                    "      Interface: {}{} ({class}, {subclass})",
                    interface.number, alternate
                ));
                for endpoint in &interface.endpoints {
                    let max_packet_size = if endpoint.transfer_type == TransferType::Isochronous {
                        format!(", Max Packet Size = {}", endpoint.max_packet_size)
                    } else {
                        String::new()
                    };
                    lines.push(format!(
                        "        Endpoint 0x{:02X}: {} {}{}",
                        endpoint.address,
                        endpoint_type_name(endpoint.transfer_type),
                        direction_name(endpoint.direction),
                        max_packet_size
                    ));
                }
            }
        }
    }
    lines.join("\n")
}

fn transport_names(
    device: &DeviceInfo,
    hci_index: Option<usize>,
    seen: &BTreeMap<(u16, u16), Vec<Option<String>>>,
) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(index) = hci_index {
        names.push(format!("usb:{index}"));
    }
    let id = (device.vendor_id, device.product_id);
    let basic = format!("usb:{:04X}:{:04X}", device.vendor_id, device.product_id);
    match seen.get(&id) {
        Some(devices) => names.push(format!("{basic}#{}", devices.len())),
        None => names.push(basic.clone()),
    }
    if let Some(serial) = &device.serial {
        let serial_seen = seen
            .get(&id)
            .is_some_and(|devices| devices.iter().any(|value| value.as_ref() == Some(serial)));
        if !serial_seen {
            names.push(format!("{basic}/{serial}"));
        }
    }
    names
}

fn run(args: Args) -> Result<String, String> {
    let devices = rusb::devices().map_err(|error| error.to_string())?;
    let mut seen = BTreeMap::<(u16, u16), Vec<Option<String>>>::new();
    let mut hci_count = 0usize;
    let mut rendered = Vec::new();
    for device in devices.iter() {
        let Ok(descriptor) = device.device_descriptor() else {
            continue;
        };
        let info = inspect_device(&device, &descriptor);
        let hci = is_bluetooth_hci(&info);
        let hci_index = hci.then_some(hci_count);
        if hci {
            hci_count += 1;
        }
        if args
            .manufacturer
            .as_ref()
            .is_some_and(|value| info.manufacturer.as_ref() != Some(value))
            || args
                .product
                .as_ref()
                .is_some_and(|value| info.product.as_ref() != Some(value))
            || (args.hci_only && !hci)
        {
            continue;
        }
        let names = transport_names(&info, hci_index, &seen);
        rendered.push(render_device(&info, &names, args.verbose));
        seen.entry((info.vendor_id, info.product_id))
            .or_default()
            .push(info.serial.clone());
    }
    Ok(rendered.join("\n\n"))
}

fn main() -> ExitCode {
    match parse_args(std::env::args()).and_then(run) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}\n{}", usage());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> DeviceInfo {
        DeviceInfo {
            vendor_id: 0x18D1,
            product_id: 0x4EE1,
            bus: 2,
            address: 7,
            class: 0,
            subclass: 0,
            protocol: 0,
            serial: Some("ABC".into()),
            manufacturer: Some("Google".into()),
            product: Some("Bluetooth Adapter".into()),
            configurations: vec![ConfigurationInfo {
                number: 1,
                interfaces: vec![InterfaceInfo {
                    number: 3,
                    setting: 1,
                    setting_count: 2,
                    class: 0xE0,
                    subclass: 1,
                    protocol: 1,
                    endpoints: vec![EndpointInfo {
                        address: 0x81,
                        transfer_type: TransferType::Isochronous,
                        direction: Direction::In,
                        max_packet_size: 64,
                    }],
                }],
            }],
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_upstream_options() {
        assert_eq!(
            parse_args(args(&[
                "probe",
                "--verbose",
                "--hci-only",
                "--manufacturer=Google",
                "--product",
                "Bluetooth Adapter",
            ])),
            Ok(Args {
                verbose: true,
                hci_only: true,
                manufacturer: Some("Google".into()),
                product: Some("Bluetooth Adapter".into()),
            })
        );
        assert!(parse_args(args(&["probe", "--manufacturer"])).is_err());
        assert!(parse_args(args(&["probe", "extra"])).is_err());
    }

    #[test]
    fn recognizes_interface_level_hci_and_renders_verbose_details() {
        let device = fixture();
        assert!(is_bluetooth_hci(&device));
        let output = render_device(&device, &["usb:0".into(), "usb:18D1:4EE1/ABC".into()], true);
        assert!(output.contains("ID 18D1:4EE1"));
        assert!(output.contains("usb:0 or usb:18D1:4EE1/ABC"));
        assert!(output.contains("Interface: 3/1 (Wireless Controller, 1/1 [Bluetooth])"));
        assert!(output.contains("Endpoint 0x81: ISOCHRONOUS IN, Max Packet Size = 64"));
    }

    #[test]
    fn transport_names_disambiguate_duplicate_ids_and_serials() {
        let device = fixture();
        let mut seen = BTreeMap::new();
        assert_eq!(
            transport_names(&device, Some(0), &seen),
            vec!["usb:0", "usb:18D1:4EE1", "usb:18D1:4EE1/ABC"]
        );
        seen.insert(
            (device.vendor_id, device.product_id),
            vec![Some("ABC".into())],
        );
        assert_eq!(
            transport_names(&device, Some(1), &seen),
            vec!["usb:1", "usb:18D1:4EE1#1"]
        );
    }
}
