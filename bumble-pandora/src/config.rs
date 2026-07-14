use serde::Deserialize;
use std::path::Path;

pub const DEFAULT_GRPC_PORT: u16 = 7999;
pub const DEFAULT_ROOTCANAL_PORT: u16 = 7300;
pub const DEFAULT_RANDOM_ADDRESS: &str = "F1:F1:F1:F1:F1:F1";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    pub io_capability: String,
    pub identity_address_type: String,
    pub pairing_sc_enable: bool,
    pub pairing_mitm_enable: bool,
    pub pairing_bonding_enable: bool,
    pub smp_local_initiator_key_distribution: u8,
    pub smp_local_responder_key_distribution: u8,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            io_capability: "no_output_no_input".into(),
            identity_address_type: "random".into(),
            pairing_sc_enable: true,
            pairing_mitm_enable: true,
            pairing_bonding_enable: true,
            smp_local_initiator_key_distribution: 0x0F,
            smp_local_responder_key_distribution: 0x0F,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PandoraConfig {
    pub transport: Option<String>,
    pub tcp: Option<String>,
    pub name: String,
    pub address: String,
    pub class_of_device: u32,
    pub keystore: Option<String>,
    pub server: ServerConfig,
}

impl Default for PandoraConfig {
    fn default() -> Self {
        Self {
            transport: None,
            tcp: None,
            name: "Bumble".into(),
            address: DEFAULT_RANDOM_ADDRESS.into(),
            class_of_device: 0,
            keystore: None,
            server: ServerConfig::default(),
        }
    }
}

impl PandoraConfig {
    pub fn from_json_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid Pandora configuration: {error}"))
    }

    pub fn transport(&self, rootcanal_port: u16, override_transport: Option<&str>) -> String {
        let transport = override_transport
            .map(str::to_owned)
            .or_else(|| self.transport.clone())
            .or_else(|| {
                self.tcp
                    .as_ref()
                    .map(|address| format!("tcp-client:{address}"))
            })
            .unwrap_or_else(|| "tcp-client:127.0.0.1:<rootcanal-port>".into());
        transport.replace("<rootcanal-port>", &rootcanal_port.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_transport_precedence_match_upstream() {
        let config = PandoraConfig::default();
        assert_eq!(
            config.transport(DEFAULT_ROOTCANAL_PORT, None),
            "tcp-client:127.0.0.1:7300"
        );
        let config: PandoraConfig = serde_json::from_str(
            r#"{"transport":"serial:/dev/tty0","tcp":"localhost:6402","server":{"pairing_mitm_enable":false}}"#,
        )
        .unwrap();
        assert_eq!(config.transport(1, None), "serial:/dev/tty0");
        assert_eq!(config.transport(1, Some("usb:0")), "usb:0");
        assert!(!config.server.pairing_mitm_enable);
        assert!(config.server.pairing_sc_enable);
    }
}
