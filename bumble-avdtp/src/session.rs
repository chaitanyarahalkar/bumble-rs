//! Sans-I/O AVDTP command handler and stream endpoint state machine.

use crate::{
    EndpointInfo, ErrorCode, MediaType, Message, ServiceCapabilities, ServiceCategory, State,
    StreamEndpointType,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEndpoint {
    pub seid: u8,
    pub media_type: MediaType,
    pub endpoint_type: StreamEndpointType,
    pub capabilities: Vec<ServiceCapabilities>,
    pub configuration: Vec<ServiceCapabilities>,
    pub state: State,
    pub remote_seid: Option<u8>,
}

impl LocalEndpoint {
    pub fn in_use(&self) -> bool {
        self.state != State::IDLE
    }

    pub fn info(&self) -> EndpointInfo {
        EndpointInfo {
            seid: self.seid,
            in_use: self.in_use(),
            media_type: self.media_type,
            endpoint_type: self.endpoint_type,
        }
    }

    fn reset(&mut self) {
        self.configuration.clear();
        self.remote_seid = None;
        self.state = State::IDLE;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    Configured { seid: u8, remote_seid: u8 },
    Reconfigured { seid: u8 },
    Opened { seid: u8 },
    Started { seid: u8 },
    Suspended { seid: u8 },
    Closed { seid: u8 },
    Aborted { seid: u8 },
    SecurityControl { seid: u8, data: Vec<u8> },
    DelayReport { seid: u8, delay: u16 },
}

#[derive(Debug, Default)]
pub struct Session {
    endpoints: Vec<LocalEndpoint>,
    events: Vec<SessionEvent>,
}

impl Session {
    pub fn add_endpoint(
        &mut self,
        media_type: MediaType,
        endpoint_type: StreamEndpointType,
        capabilities: Vec<ServiceCapabilities>,
    ) -> u8 {
        let seid = u8::try_from(self.endpoints.len() + 1).expect("at most 255 endpoints");
        self.endpoints.push(LocalEndpoint {
            seid,
            media_type,
            endpoint_type,
            capabilities,
            configuration: Vec::new(),
            state: State::IDLE,
            remote_seid: None,
        });
        seid
    }

    pub fn endpoints(&self) -> &[LocalEndpoint] {
        &self.endpoints
    }

    pub fn endpoint(&self, seid: u8) -> Option<&LocalEndpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.seid == seid)
    }

    pub fn take_events(&mut self) -> Vec<SessionEvent> {
        core::mem::take(&mut self.events)
    }

    pub fn handle_command(&mut self, command: Message) -> Message {
        use Message::*;

        match command {
            DiscoverCommand => DiscoverResponse {
                endpoints: self.endpoints.iter().map(LocalEndpoint::info).collect(),
            },
            GetCapabilitiesCommand { acp_seid } => self
                .endpoint(acp_seid)
                .map(|endpoint| GetCapabilitiesResponse {
                    capabilities: endpoint.capabilities.clone(),
                })
                .unwrap_or(GetCapabilitiesReject {
                    error_code: ErrorCode::BAD_ACP_SEID,
                }),
            GetAllCapabilitiesCommand { acp_seid } => self
                .endpoint(acp_seid)
                .map(|endpoint| GetAllCapabilitiesResponse {
                    capabilities: endpoint.capabilities.clone(),
                })
                .unwrap_or(GetAllCapabilitiesReject {
                    error_code: ErrorCode::BAD_ACP_SEID,
                }),
            SetConfigurationCommand {
                acp_seid,
                int_seid,
                capabilities,
            } => {
                let Some(endpoint) = self.endpoint_mut(acp_seid) else {
                    return SetConfigurationReject {
                        service_category: ServiceCategory(0),
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                };
                if endpoint.in_use() {
                    return SetConfigurationReject {
                        service_category: ServiceCategory(0),
                        error_code: ErrorCode::SEP_IN_USE,
                    };
                }
                endpoint.configuration = capabilities;
                endpoint.remote_seid = Some(int_seid);
                endpoint.state = State::CONFIGURED;
                self.events.push(SessionEvent::Configured {
                    seid: acp_seid,
                    remote_seid: int_seid,
                });
                SetConfigurationResponse
            }
            GetConfigurationCommand { acp_seid } => {
                let Some(endpoint) = self.endpoint(acp_seid) else {
                    return GetConfigurationReject {
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                };
                if !matches!(
                    endpoint.state,
                    State::CONFIGURED | State::OPEN | State::STREAMING
                ) {
                    return GetConfigurationReject {
                        error_code: ErrorCode::BAD_STATE,
                    };
                }
                GetConfigurationResponse {
                    capabilities: endpoint.configuration.clone(),
                }
            }
            ReconfigureCommand {
                acp_seid,
                capabilities,
            } => {
                let Some(endpoint) = self.endpoint_mut(acp_seid) else {
                    return ReconfigureReject {
                        service_category: ServiceCategory(0),
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                };
                if endpoint.state != State::OPEN {
                    return ReconfigureReject {
                        service_category: ServiceCategory(0),
                        error_code: ErrorCode::BAD_STATE,
                    };
                }
                endpoint.configuration = capabilities;
                self.events
                    .push(SessionEvent::Reconfigured { seid: acp_seid });
                ReconfigureResponse
            }
            OpenCommand { acp_seid } => {
                if !self.transition(acp_seid, State::CONFIGURED, State::OPEN) {
                    return self.simple_reject(
                        acp_seid,
                        OpenReject {
                            error_code: ErrorCode::BAD_STATE,
                        },
                    );
                }
                self.events.push(SessionEvent::Opened { seid: acp_seid });
                OpenResponse
            }
            StartCommand { acp_seids } => {
                if let Some((seid, error_code)) = self.validate_all(&acp_seids, State::OPEN) {
                    return StartReject {
                        acp_seid: seid,
                        error_code,
                    };
                }
                for seid in acp_seids {
                    self.endpoint_mut(seid).expect("validated endpoint").state = State::STREAMING;
                    self.events.push(SessionEvent::Started { seid });
                }
                StartResponse
            }
            SuspendCommand { acp_seids } => {
                if let Some((seid, error_code)) = self.validate_all(&acp_seids, State::STREAMING) {
                    return SuspendReject {
                        acp_seid: seid,
                        error_code,
                    };
                }
                for seid in acp_seids {
                    self.endpoint_mut(seid).expect("validated endpoint").state = State::OPEN;
                    self.events.push(SessionEvent::Suspended { seid });
                }
                SuspendResponse
            }
            CloseCommand { acp_seid } => {
                let Some(endpoint) = self.endpoint_mut(acp_seid) else {
                    return CloseReject {
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                };
                if !matches!(endpoint.state, State::OPEN | State::STREAMING) {
                    return CloseReject {
                        error_code: ErrorCode::BAD_STATE,
                    };
                }
                endpoint.reset();
                self.events.push(SessionEvent::Closed { seid: acp_seid });
                CloseResponse
            }
            AbortCommand { acp_seid } => {
                if let Some(endpoint) = self.endpoint_mut(acp_seid) {
                    endpoint.reset();
                    self.events.push(SessionEvent::Aborted { seid: acp_seid });
                }
                // Upstream deliberately accepts abort for an unknown/idle SEP.
                AbortResponse
            }
            SecurityControlCommand { acp_seid, data } => {
                if self.endpoint(acp_seid).is_none() {
                    return SecurityControlReject {
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                }
                if self
                    .endpoint(acp_seid)
                    .is_some_and(|endpoint| !endpoint.in_use())
                {
                    return SecurityControlReject {
                        error_code: ErrorCode::BAD_STATE,
                    };
                }
                self.events.push(SessionEvent::SecurityControl {
                    seid: acp_seid,
                    data,
                });
                SecurityControlResponse
            }
            DelayReportCommand { acp_seid, delay } => {
                if self.endpoint(acp_seid).is_none() {
                    return DelayReportReject {
                        error_code: ErrorCode::BAD_ACP_SEID,
                    };
                }
                if self
                    .endpoint(acp_seid)
                    .is_some_and(|endpoint| !endpoint.in_use())
                {
                    return DelayReportReject {
                        error_code: ErrorCode::BAD_STATE,
                    };
                }
                self.events.push(SessionEvent::DelayReport {
                    seid: acp_seid,
                    delay,
                });
                DelayReportResponse
            }
            _ => GeneralReject,
        }
    }

    fn endpoint_mut(&mut self, seid: u8) -> Option<&mut LocalEndpoint> {
        self.endpoints
            .iter_mut()
            .find(|endpoint| endpoint.seid == seid)
    }

    fn transition(&mut self, seid: u8, from: State, to: State) -> bool {
        let Some(endpoint) = self.endpoint_mut(seid) else {
            return false;
        };
        if endpoint.state != from {
            return false;
        }
        endpoint.state = to;
        true
    }

    fn simple_reject(&self, seid: u8, bad_state: Message) -> Message {
        if self.endpoint(seid).is_none() {
            match bad_state {
                Message::OpenReject { .. } => Message::OpenReject {
                    error_code: ErrorCode::BAD_ACP_SEID,
                },
                _ => bad_state,
            }
        } else {
            bad_state
        }
    }

    fn validate_all(&self, seids: &[u8], state: State) -> Option<(u8, ErrorCode)> {
        for seid in seids {
            let Some(endpoint) = self.endpoint(*seid) else {
                return Some((*seid, ErrorCode::BAD_ACP_SEID));
            };
            if endpoint.state != state {
                return Some((*seid, ErrorCode::BAD_STATE));
            }
        }
        None
    }
}
