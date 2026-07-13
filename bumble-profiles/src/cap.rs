//! Common Audio Profile (CAP) service and CSIS inclusion helper.

use crate::csip::{CoordinatedSetIdentificationService, COORDINATED_SET_IDENTIFICATION_SERVICE};
use crate::{discover_profile, uuid, Result};
use bumble_gatt::{AttTransport, GattClient, ServiceDefinition, ServiceProxy};

pub const COMMON_AUDIO_SERVICE: u16 = 0x1853;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommonAudioService {
    pub coordinated_set_service_index: usize,
}

impl CommonAudioService {
    pub fn new(coordinated_set_service_index: usize) -> Self {
        Self {
            coordinated_set_service_index,
        }
    }

    pub fn definition(self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: uuid(COMMON_AUDIO_SERVICE),
            primary: true,
            included_services: vec![self.coordinated_set_service_index],
            characteristics: Vec::new(),
        }
    }

    /// Build the CSIS definition followed by a CAS definition that includes it.
    pub fn definitions(
        coordinated_set_service: &CoordinatedSetIdentificationService,
    ) -> Vec<ServiceDefinition> {
        vec![
            coordinated_set_service.definition(),
            Self::new(0).definition(),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct CommonAudioServiceProxy {
    pub service: ServiceProxy,
}

impl CommonAudioServiceProxy {
    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, _)) = discover_profile(client, transport, COMMON_AUDIO_SERVICE)? else {
            return Ok(None);
        };
        Ok(Some(Self { service }))
    }

    pub fn discover_coordinated_set_service(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<ServiceProxy>> {
        Ok(client
            .discover_included_services(transport, &self.service)?
            .into_iter()
            .find(|service| service.uuid == uuid(COORDINATED_SET_IDENTIFICATION_SERVICE)))
    }
}
