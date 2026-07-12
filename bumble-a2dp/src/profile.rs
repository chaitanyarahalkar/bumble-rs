//! High-level A2DP initiator over the sans-I/O AVDTP/L2CAP runtime.

use core::fmt;

use bumble_avdtp::l2cap::L2capSession;
use bumble_avdtp::{
    EndpointInfo, Message, ServiceCapabilities, ServiceCategory, StreamEndpointType,
};
use bumble_l2cap::ChannelManager;

use crate::MediaCodecInformation;

#[derive(Debug)]
pub enum Error {
    Signaling(bumble_avdtp::l2cap::BindingError),
    Codec(crate::Error),
    Protocol(&'static str),
    Drive(String),
    Timeout,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl From<bumble_avdtp::l2cap::BindingError> for Error {
    fn from(value: bumble_avdtp::l2cap::BindingError) -> Self {
        Self::Signaling(value)
    }
}

impl From<crate::Error> for Error {
    fn from(value: crate::Error) -> Self {
        Self::Codec(value)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEndpoint {
    pub info: EndpointInfo,
    pub capabilities: Vec<ServiceCapabilities>,
}

impl RemoteEndpoint {
    pub fn supports(&self, codec: &MediaCodecInformation) -> Result<bool> {
        let desired_type = codec.codec_type().0;
        let desired_data = codec.to_bytes()?;
        Ok(self.capabilities.iter().any(|capability| match capability {
            ServiceCapabilities::MediaCodec {
                media_codec_type,
                media_codec_information,
                ..
            } if *media_codec_type == desired_type => {
                desired_type != 0xFF || media_codec_information.get(..6) == desired_data.get(..6)
            }
            _ => false,
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamHandle {
    pub local_seid: u8,
    pub remote_seid: u8,
}

pub struct A2dpClient<'a, Drive>
where
    Drive: FnMut(&mut ChannelManager, &mut L2capSession) -> Result<()>,
{
    manager: &'a mut ChannelManager,
    signaling: &'a mut L2capSession,
    drive: Drive,
    drive_limit: usize,
}

impl<'a, Drive> A2dpClient<'a, Drive>
where
    Drive: FnMut(&mut ChannelManager, &mut L2capSession) -> Result<()>,
{
    pub fn new(
        manager: &'a mut ChannelManager,
        signaling: &'a mut L2capSession,
        drive: Drive,
    ) -> Self {
        Self {
            manager,
            signaling,
            drive,
            drive_limit: 128,
        }
    }

    pub fn discover(&mut self) -> Result<Vec<RemoteEndpoint>> {
        let Message::DiscoverResponse { endpoints } = self.request(Message::DiscoverCommand)?
        else {
            return Err(Error::Protocol("unexpected discover response"));
        };
        let mut discovered = Vec::with_capacity(endpoints.len());
        for info in endpoints {
            let Message::GetAllCapabilitiesResponse { capabilities } =
                self.request(Message::GetAllCapabilitiesCommand {
                    acp_seid: info.seid,
                })?
            else {
                return Err(Error::Protocol("capability discovery rejected"));
            };
            discovered.push(RemoteEndpoint { info, capabilities });
        }
        Ok(discovered)
    }

    pub fn find_compatible_sink<'b>(
        &self,
        endpoints: &'b [RemoteEndpoint],
        codec: &MediaCodecInformation,
    ) -> Result<Option<&'b RemoteEndpoint>> {
        for endpoint in endpoints {
            if !endpoint.info.in_use
                && endpoint.info.endpoint_type == StreamEndpointType::SINK
                && endpoint
                    .capabilities
                    .iter()
                    .any(|capability| capability.category() == ServiceCategory::MEDIA_TRANSPORT)
                && endpoint.supports(codec)?
            {
                return Ok(Some(endpoint));
            }
        }
        Ok(None)
    }

    pub fn configure_open_start(
        &mut self,
        local_seid: u8,
        remote: &RemoteEndpoint,
        codec: &MediaCodecInformation,
    ) -> Result<StreamHandle> {
        if !remote.supports(codec)? {
            return Err(Error::Protocol("remote endpoint does not support codec"));
        }
        let configuration = vec![
            ServiceCapabilities::empty(ServiceCategory::MEDIA_TRANSPORT),
            codec.to_avdtp_capability()?,
        ];
        self.expect(
            Message::SetConfigurationCommand {
                acp_seid: remote.info.seid,
                int_seid: local_seid,
                capabilities: configuration,
            },
            Message::SetConfigurationResponse,
        )?;
        self.expect(
            Message::OpenCommand {
                acp_seid: remote.info.seid,
            },
            Message::OpenResponse,
        )?;
        self.expect(
            Message::StartCommand {
                acp_seids: vec![remote.info.seid],
            },
            Message::StartResponse,
        )?;
        Ok(StreamHandle {
            local_seid,
            remote_seid: remote.info.seid,
        })
    }

    pub fn suspend(&mut self, stream: StreamHandle) -> Result<()> {
        self.expect(
            Message::SuspendCommand {
                acp_seids: vec![stream.remote_seid],
            },
            Message::SuspendResponse,
        )
    }

    pub fn start(&mut self, stream: StreamHandle) -> Result<()> {
        self.expect(
            Message::StartCommand {
                acp_seids: vec![stream.remote_seid],
            },
            Message::StartResponse,
        )
    }

    pub fn close(&mut self, stream: StreamHandle) -> Result<()> {
        self.expect(
            Message::CloseCommand {
                acp_seid: stream.remote_seid,
            },
            Message::CloseResponse,
        )
    }

    fn expect(&mut self, command: Message, expected: Message) -> Result<()> {
        if self.request(command)? == expected {
            Ok(())
        } else {
            Err(Error::Protocol("AVDTP command rejected"))
        }
    }

    fn request(&mut self, command: Message) -> Result<Message> {
        let label = self.signaling.send_command(self.manager, command)?;
        for _ in 0..self.drive_limit {
            (self.drive)(self.manager, self.signaling)?;
            if let Some(response) = self.signaling.take_response(label) {
                return Ok(response);
            }
        }
        Err(Error::Timeout)
    }
}
