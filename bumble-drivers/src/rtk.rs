//! Realtek USB controller firmware support.

use crate::{
    metadata_u16, require_success, CommandResponse, DriverHost, Error, FirmwareProvider,
    HciMetadata, Result,
};
use bumble_hci::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const RTK_ROM_LMP_8723A: u16 = 0x1200;
pub const RTK_ROM_LMP_8723B: u16 = 0x8723;
pub const RTK_ROM_LMP_8821A: u16 = 0x8821;
pub const RTK_ROM_LMP_8761A: u16 = 0x8761;
pub const RTK_ROM_LMP_8822B: u16 = 0x8822;
pub const RTK_ROM_LMP_8852A: u16 = 0x8852;
pub const RTK_CONFIG_MAGIC: u32 = 0x8723_AB55;
pub const RTK_EPATCH_SIGNATURE: &[u8; 8] = b"Realtech";
pub const RTK_EXTENSION_SIGNATURE: [u8; 4] = [0x51, 0x04, 0xFD, 0x77];
pub const RTK_FRAGMENT_LENGTH: usize = 252;
pub const RTK_FIRMWARE_DIR_ENV: &str = "BUMBLE_RTK_FIRMWARE_DIR";
pub const RTK_LINUX_FIRMWARE_DIR: &str = "/lib/firmware/rtl_bt";

pub const HCI_RTK_READ_ROM_VERSION_COMMAND: u16 = 0xFC6D;
pub const HCI_RTK_DOWNLOAD_COMMAND: u16 = 0xFC20;
pub const HCI_RTK_DROP_FIRMWARE_COMMAND: u16 = 0xFC66;

pub const RTK_USB_PRODUCTS: &[(u16, u16)] = &[
    // 8723AE
    (0x0930, 0x021D),
    (0x13D3, 0x3394),
    // 8723BE
    (0x0489, 0xE085),
    (0x0489, 0xE08B),
    (0x04F2, 0xB49F),
    (0x13D3, 0x3410),
    (0x13D3, 0x3416),
    (0x13D3, 0x3459),
    (0x13D3, 0x3494),
    // 8723BU / 8723DE
    (0x7392, 0xA611),
    (0x0BDA, 0xB009),
    (0x2FF8, 0xB011),
    // 8761BUV
    (0x0B05, 0x190E),
    (0x0BDA, 0x8771),
    (0x0BDA, 0x877B),
    (0x0BDA, 0xA728),
    (0x0BDA, 0xA729),
    (0x2230, 0x0016),
    (0x2357, 0x0604),
    (0x2550, 0x8761),
    (0x2B89, 0x8761),
    (0x2C0A, 0x8761),
    (0x7392, 0xC611),
    // 8761CUV
    (0x0B05, 0x1BF6),
    (0x0BDA, 0xC761),
    (0x7392, 0xF611),
    // 8821AE / 8821CE
    (0x0B05, 0x17DC),
    (0x13D3, 0x3414),
    (0x13D3, 0x3458),
    (0x13D3, 0x3461),
    (0x13D3, 0x3462),
    (0x0BDA, 0xB00C),
    (0x0BDA, 0xC822),
    (0x13D3, 0x3529),
    // 8822BE / 8822CE / 8822CU
    (0x0B05, 0x185C),
    (0x13D3, 0x3526),
    (0x04C5, 0x161F),
    (0x04CA, 0x4005),
    (0x0B05, 0x18EF),
    (0x0BDA, 0xC123),
    (0x0CB5, 0xC547),
    (0x1358, 0xC123),
    (0x13D3, 0x3548),
    (0x13D3, 0x3549),
    (0x13D3, 0x3553),
    (0x13D3, 0x3555),
    (0x2FF8, 0x3051),
    // 8852AE / 8852BE / 8852CE
    (0x04C5, 0x165C),
    (0x04CA, 0x4006),
    (0x0BDA, 0x2852),
    (0x0BDA, 0x385A),
    (0x0BDA, 0x4852),
    (0x0BDA, 0xC852),
    (0x0CB8, 0xC549),
    (0x0BDA, 0x887B),
    (0x0CB8, 0xC559),
    (0x13D3, 0x3571),
    (0x04C5, 0x1675),
    (0x04CA, 0x4007),
    (0x0CB8, 0xC558),
    (0x13D3, 0x3586),
    (0x13D3, 0x3587),
    (0x13D3, 0x3592),
];

pub fn project_rom(project_id: u8) -> Option<u16> {
    Some(match project_id {
        0 => RTK_ROM_LMP_8723A,
        1 | 9 => RTK_ROM_LMP_8723B,
        2 | 10 => RTK_ROM_LMP_8821A,
        3 | 14 | 51 => RTK_ROM_LMP_8761A,
        8 | 13 => RTK_ROM_LMP_8822B,
        18 | 20 | 25 => RTK_ROM_LMP_8852A,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    pub chip_id: u16,
    pub payload: Vec<u8>,
    pub svn_version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Firmware {
    pub project_id: u8,
    pub version: u32,
    pub patches: Vec<Patch>,
}

impl Firmware {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        const HEADER_SIZE: usize = 14;
        if !bytes.starts_with(RTK_EPATCH_SIGNATURE) {
            return Err(Error::InvalidFirmware(
                "Realtek firmware does not start with epatch signature".into(),
            ));
        }
        if !bytes.ends_with(&RTK_EXTENSION_SIGNATURE) {
            return Err(Error::InvalidFirmware(
                "Realtek firmware does not end with extension signature".into(),
            ));
        }
        if bytes.len() < HEADER_SIZE {
            return Err(Error::InvalidFirmware(
                "Realtek firmware is shorter than its header".into(),
            ));
        }

        let mut extension_offset = bytes.len() - RTK_EXTENSION_SIGNATURE.len();
        let mut project_id = None;
        while extension_offset >= HEADER_SIZE + 2 {
            let length = usize::from(bytes[extension_offset - 2]);
            let opcode = bytes[extension_offset - 1];
            extension_offset -= 2;
            if opcode == 0xFF {
                break;
            }
            if length == 0 {
                return Err(Error::InvalidFirmware(
                    "Realtek extension contains a zero-length instruction".into(),
                ));
            }
            if extension_offset < length {
                return Err(Error::InvalidFirmware(
                    "Realtek extension instruction runs before the header".into(),
                ));
            }
            if opcode == 0 && length == 1 {
                project_id = Some(bytes[extension_offset - 1]);
                break;
            }
            extension_offset -= length;
        }
        let project_id = project_id.ok_or_else(|| {
            Error::InvalidFirmware("Realtek firmware project ID not found".into())
        })?;

        let version = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .expect("validated Realtek header version"),
        );
        let patch_count = usize::from(u16::from_le_bytes(
            bytes[12..14]
                .try_into()
                .expect("validated Realtek patch count"),
        ));
        let tables_end = HEADER_SIZE
            .checked_add(patch_count.checked_mul(8).ok_or_else(|| {
                Error::InvalidFirmware("Realtek patch table length overflow".into())
            })?)
            .ok_or_else(|| Error::InvalidFirmware("Realtek patch table overflow".into()))?;
        if tables_end > bytes.len() {
            return Err(Error::InvalidFirmware(
                "Realtek firmware patch tables are truncated".into(),
            ));
        }

        let chip_ids_offset = HEADER_SIZE;
        let patch_lengths_offset = chip_ids_offset + 2 * patch_count;
        let patch_offsets_offset = chip_ids_offset + 4 * patch_count;
        let mut patches = Vec::with_capacity(patch_count);
        for index in 0..patch_count {
            let chip_id = read_u16(bytes, chip_ids_offset + 2 * index)?;
            let patch_length = usize::from(read_u16(bytes, patch_lengths_offset + 2 * index)?);
            let patch_offset = usize::try_from(read_u32(bytes, patch_offsets_offset + 4 * index)?)
                .map_err(|_| Error::InvalidFirmware("Realtek patch offset is too large".into()))?;
            let patch_end = patch_offset
                .checked_add(patch_length)
                .ok_or_else(|| Error::InvalidFirmware("Realtek patch length overflow".into()))?;
            if patch_length < 8 || patch_end > bytes.len() {
                return Err(Error::InvalidFirmware(
                    "Realtek firmware patch is truncated".into(),
                ));
            }
            let svn_version = read_u32(bytes, patch_end - 8)?;
            let mut payload = bytes[patch_offset..patch_end - 4].to_vec();
            payload.extend_from_slice(&version.to_le_bytes());
            patches.push(Patch {
                chip_id,
                payload,
                svn_version,
            });
        }

        Ok(Self {
            project_id,
            version,
            patches,
        })
    }

    pub fn patch_for_rom_version(&self, rom_version: u8) -> Option<&Patch> {
        let chip_id = u16::from(rom_version) + 1;
        self.patches.iter().find(|patch| patch.chip_id == chip_id)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| Error::InvalidFirmware("truncated Realtek u16 field".into()))?;
    Ok(u16::from_le_bytes(
        value.try_into().expect("validated Realtek u16 field"),
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| Error::InvalidFirmware("truncated Realtek u32 field".into()))?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("validated Realtek u32 field"),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverInfo {
    pub rom: u16,
    /// `(HCI subversion, HCI version)`; version zero is a wildcard.
    pub hci: (u16, u8),
    pub config_needed: bool,
    pub has_rom_version: bool,
    pub has_msft_ext: bool,
    pub firmware_name: &'static str,
    pub config_name: &'static str,
}

pub const DRIVER_INFOS: &[DriverInfo] = &[
    DriverInfo {
        rom: RTK_ROM_LMP_8723A,
        hci: (0x0B, 0x06),
        config_needed: false,
        has_rom_version: false,
        has_msft_ext: false,
        firmware_name: "rtl8723a_fw.bin",
        config_name: "",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8723B,
        hci: (0x0B, 0x06),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8723b_fw.bin",
        config_name: "rtl8723b_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8723B,
        hci: (0x0D, 0x08),
        config_needed: true,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8723d_fw.bin",
        config_name: "rtl8723d_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8821A,
        hci: (0x0A, 0x06),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8821a_fw.bin",
        config_name: "rtl8821a_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8821A,
        hci: (0x0C, 0x08),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8821c_fw.bin",
        config_name: "rtl8821c_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8761A,
        hci: (0x0A, 0x06),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8761a_fw.bin",
        config_name: "rtl8761a_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8761A,
        hci: (0x0B, 0x0A),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8761bu_fw.bin",
        config_name: "rtl8761bu_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8761A,
        hci: (0x0E, 0x00),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: false,
        firmware_name: "rtl8761cu_fw.bin",
        config_name: "rtl8761cu_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8822B,
        hci: (0x0C, 0x0A),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8822cu_fw.bin",
        config_name: "rtl8822cu_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8822B,
        hci: (0x0B, 0x07),
        config_needed: true,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8822b_fw.bin",
        config_name: "rtl8822b_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8852A,
        hci: (0x0A, 0x0B),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8852au_fw.bin",
        config_name: "rtl8852au_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8852A,
        hci: (0x0B, 0x0B),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8852bu_fw.bin",
        config_name: "rtl8852bu_config.bin",
    },
    DriverInfo {
        rom: RTK_ROM_LMP_8852A,
        hci: (0x0C, 0x0C),
        config_needed: false,
        has_rom_version: true,
        has_msft_ext: true,
        firmware_name: "rtl8852cu_fw.bin",
        config_name: "rtl8852cu_config.bin",
    },
];

pub fn find_driver_info(
    hci_version: u8,
    hci_subversion: u16,
    lmp_subversion: u16,
) -> Option<&'static DriverInfo> {
    DRIVER_INFOS.iter().find(|info| {
        info.rom == lmp_subversion
            && info.hci.0 == hci_subversion
            && (info.hci.1 == hci_version || info.hci.1 == 0)
    })
}

pub fn check(metadata: &HciMetadata) -> bool {
    if metadata.get("driver").is_some_and(|driver| driver == "rtk") {
        return true;
    }
    let Some(vendor_id) = metadata_u16(metadata, "vendor_id") else {
        return false;
    };
    let Some(product_id) = metadata_u16(metadata, "product_id") else {
        return false;
    };
    RTK_USB_PRODUCTS.contains(&(vendor_id, product_id))
}

#[derive(Clone, Debug, Default)]
pub struct FirmwareSearch {
    pub environment_directory: Option<PathBuf>,
    pub project_directory: Option<PathBuf>,
    pub package_directory: Option<PathBuf>,
    pub system_directory: Option<PathBuf>,
    pub current_directory: Option<PathBuf>,
}

impl FirmwareSearch {
    pub fn from_environment(
        project_directory: Option<PathBuf>,
        package_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            environment_directory: std::env::var_os(RTK_FIRMWARE_DIR_ENV).map(PathBuf::from),
            project_directory,
            package_directory,
            system_directory: cfg!(target_os = "linux")
                .then(|| PathBuf::from(RTK_LINUX_FIRMWARE_DIR)),
            current_directory: std::env::current_dir().ok(),
        }
    }

    pub fn find(&self, file_name: &str) -> Option<PathBuf> {
        if let Some(directory) = &self.environment_directory {
            return existing_file(directory, file_name);
        }
        [
            self.project_directory.as_ref(),
            self.package_directory.as_ref(),
            self.system_directory.as_ref(),
            self.current_directory.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find_map(|directory| existing_file(directory, file_name))
    }
}

fn existing_file(directory: &Path, file_name: &str) -> Option<PathBuf> {
    let path = directory.join(file_name);
    path.is_file().then_some(path)
}

impl FirmwareProvider for FirmwareSearch {
    fn load(&self, file_name: &str) -> Result<Option<Vec<u8>>> {
        self.find(file_name)
            .map(fs::read)
            .transpose()
            .map_err(Into::into)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalVersion {
    pub hci_version: u8,
    pub hci_subversion: u16,
    pub lmp_version: u8,
    pub company_identifier: u16,
    pub lmp_subversion: u16,
}

pub fn parse_local_version(response: &CommandResponse) -> Result<LocalVersion> {
    require_success(response, "read local version")?;
    let bytes = &response.return_parameters;
    if bytes.len() != 9 {
        return Err(Error::InvalidResponse(format!(
            "local version has {} bytes, expected 9",
            bytes.len()
        )));
    }
    Ok(LocalVersion {
        hci_version: bytes[1],
        hci_subversion: u16::from_le_bytes([bytes[2], bytes[3]]),
        lmp_version: bytes[4],
        company_identifier: u16::from_le_bytes([bytes[5], bytes[6]]),
        lmp_subversion: u16::from_le_bytes([bytes[7], bytes[8]]),
    })
}

pub fn read_rom_version_command() -> Command {
    Command::Generic {
        op_code: HCI_RTK_READ_ROM_VERSION_COMMAND,
        parameters: Vec::new(),
    }
}

pub fn drop_firmware_command() -> Command {
    Command::Generic {
        op_code: HCI_RTK_DROP_FIRMWARE_COMMAND,
        parameters: Vec::new(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadFragment {
    pub index: u8,
    pub payload: Vec<u8>,
}

impl DownloadFragment {
    pub fn command(&self) -> Command {
        let mut parameters = Vec::with_capacity(1 + self.payload.len());
        parameters.push(self.index);
        parameters.extend_from_slice(&self.payload);
        Command::Generic {
            op_code: HCI_RTK_DOWNLOAD_COMMAND,
            parameters,
        }
    }
}

pub fn download_fragments(payload: &[u8]) -> Vec<DownloadFragment> {
    let fragment_count = payload.len().div_ceil(RTK_FRAGMENT_LENGTH);
    (0..fragment_count)
        .map(|fragment_index| {
            // Preserve Bumble's current index calculation exactly. The 7-bit
            // index wraps; the final fragment adds the high-bit end marker.
            let mut index = (fragment_index & 0x7F) as u8;
            if fragment_index + 1 == fragment_count {
                index |= 0x80;
            }
            let offset = fragment_index * RTK_FRAGMENT_LENGTH;
            DownloadFragment {
                index,
                payload: payload[offset..(offset + RTK_FRAGMENT_LENGTH).min(payload.len())]
                    .to_vec(),
            }
        })
        .collect()
}

pub struct Driver {
    info: &'static DriverInfo,
    firmware: Vec<u8>,
    config: Option<Vec<u8>>,
}

impl Driver {
    pub const POST_RESET_DELAY: Duration = Duration::from_millis(200);
    pub const POST_DROP_DELAY: Duration = Duration::from_millis(200);

    pub fn for_host(
        host: &mut impl DriverHost,
        provider: &impl FirmwareProvider,
        force: bool,
    ) -> Result<Option<Self>> {
        if !force && !check(host.metadata()) {
            return Ok(None);
        }
        let Some(info) = Self::driver_info_for_host(host)? else {
            return Ok(None);
        };
        let Some(firmware) = provider.load(info.firmware_name)? else {
            return Ok(None);
        };
        let config = if info.config_name.is_empty() {
            None
        } else {
            provider
                .load(info.config_name)?
                .filter(|config| !config.is_empty())
        };
        if info.config_needed && config.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            info,
            firmware,
            config,
        }))
    }

    pub fn info(&self) -> &'static DriverInfo {
        self.info
    }

    pub fn driver_info_for_host(host: &mut impl DriverHost) -> Result<Option<&'static DriverInfo>> {
        match host.transact_with_timeout(Command::Reset, Self::POST_RESET_DELAY) {
            Ok(response) => require_success(&response, "Realtek reset")?,
            Err(Error::Timeout(_)) => {
                let response = host.transact(Command::Reset)?;
                require_success(&response, "Realtek reset retry")?;
            }
            Err(error) => return Err(error),
        }

        let response = host.transact(Command::ReadLocalVersionInformation)?;
        let version = match parse_local_version(&response) {
            Ok(version) => version,
            Err(Error::InvalidResponse(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(find_driver_info(
            version.hci_version,
            version.hci_subversion,
            version.lmp_subversion,
        ))
    }

    pub fn loaded_firmware_version(host: &mut impl DriverHost) -> Result<Option<u32>> {
        let Some(_rom_version) = read_rom_version(host)? else {
            return Ok(None);
        };
        let response = host.transact(Command::ReadLocalVersionInformation)?;
        let version = parse_local_version(&response)?;
        Ok(Some(
            (u32::from(version.hci_subversion) << 16) | u32::from(version.lmp_subversion),
        ))
    }

    pub fn drop_firmware(host: &mut impl DriverHost) -> Result<()> {
        host.send_without_response(drop_firmware_command())?;
        host.delay(Self::POST_DROP_DELAY);
        Ok(())
    }

    pub fn init_controller(&self, host: &mut impl DriverHost) -> Result<InitOutcome> {
        let firmware_version = self.download_firmware(host)?;
        let response = host.transact(Command::Reset)?;
        require_success(&response, "Realtek post-download reset")?;
        Ok(InitOutcome {
            firmware_name: self.info.firmware_name,
            firmware_version,
        })
    }

    pub fn download_firmware(&self, host: &mut impl DriverHost) -> Result<Option<u32>> {
        if self.info.rom == RTK_ROM_LMP_8723A {
            // This intentionally mirrors upstream Bumble: legacy 8723A images
            // are recognized, but their download routine remains a no-op.
            return Ok(None);
        }
        if !matches!(
            self.info.rom,
            RTK_ROM_LMP_8723B
                | RTK_ROM_LMP_8821A
                | RTK_ROM_LMP_8761A
                | RTK_ROM_LMP_8822B
                | RTK_ROM_LMP_8852A
        ) {
            return Err(Error::Unsupported(format!(
                "Realtek ROM 0x{:04X}",
                self.info.rom
            )));
        }

        let rom_version = if self.info.has_rom_version {
            let Some(version) = read_rom_version(host)? else {
                return Ok(None);
            };
            version
        } else {
            0
        };
        let firmware = Firmware::parse(&self.firmware)?;
        let Some(patch) = firmware.patch_for_rom_version(rom_version) else {
            return Ok(None);
        };
        let mut payload = patch.payload.clone();
        if let Some(config) = &self.config {
            payload.extend_from_slice(config);
        }
        let fragments = download_fragments(&payload);
        for fragment in fragments {
            let expected_index = fragment.index;
            let response = host.transact(fragment.command())?;
            require_success(&response, "Realtek firmware download")?;
            if response.return_parameters.get(1).copied() != Some(expected_index) {
                return Err(Error::InvalidResponse(format!(
                    "Realtek download acknowledged index {:?}, expected 0x{expected_index:02X}",
                    response.return_parameters.get(1)
                )));
            }
        }

        // Upstream re-reads and logs this value, but a failed diagnostic read
        // does not invalidate an otherwise acknowledged download.
        let _ = read_rom_version(host);
        Ok(Some(firmware.version))
    }
}

fn read_rom_version(host: &mut impl DriverHost) -> Result<Option<u8>> {
    let response = host.transact(read_rom_version_command())?;
    if response.status() != Some(0) || response.return_parameters.len() != 2 {
        return Ok(None);
    }
    Ok(Some(response.return_parameters[1]))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitOutcome {
    pub firmware_name: &'static str,
    pub firmware_version: Option<u32>,
}
