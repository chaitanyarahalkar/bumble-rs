//! HFP eSCO parameter presets and HCI command construction.

use bumble::Address;
use bumble_hci::{CodingFormat, Command};

use crate::AudioCodec;

const EV3: u16 = 0x0008;
const HV1: u16 = 0x0001;
const HV3: u16 = 0x0004;
const NO_2_EV3: u16 = 0x0040;
const NO_3_EV3: u16 = 0x0080;
const NO_2_EV5: u16 = 0x0100;
const NO_3_EV5: u16 = 0x0200;
const COMMON_PACKET_MASK: u16 = EV3 | NO_2_EV3 | NO_3_EV3 | NO_2_EV5 | NO_3_EV5;
const REDUCED_PACKET_MASK: u16 = EV3 | NO_3_EV3 | NO_2_EV5 | NO_3_EV5;

/// The eight codec parameter sets defined by HFP 1.8 section 5.7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultCodecParameters {
    ScoCvsdD0,
    ScoCvsdD1,
    EscoCvsdS1,
    EscoCvsdS2,
    EscoCvsdS3,
    EscoCvsdS4,
    EscoMsbcT1,
    EscoMsbcT2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EscoParameters {
    pub transmit_coding_format: CodingFormat,
    pub receive_coding_format: CodingFormat,
    pub max_latency: u16,
    pub packet_type: u16,
    pub retransmission_effort: u8,
    pub input_bandwidth: u32,
    pub output_bandwidth: u32,
}

impl EscoParameters {
    pub fn for_default(default: DefaultCodecParameters) -> Self {
        use DefaultCodecParameters::*;

        match default {
            ScoCvsdD0 => Self::new(0x02, 0xFFFF, HV1, 0x00, 16_000),
            ScoCvsdD1 => Self::new(0x02, 0xFFFF, HV3, 0x00, 16_000),
            EscoCvsdS1 => Self::cvsd_s1(),
            EscoCvsdS2 => Self::new(0x02, 0x0007, REDUCED_PACKET_MASK, 0x01, 16_000),
            EscoCvsdS3 => Self::new(0x02, 0x000A, REDUCED_PACKET_MASK, 0x01, 16_000),
            EscoCvsdS4 => Self::new(0x02, 0x000C, REDUCED_PACKET_MASK, 0x02, 16_000),
            EscoMsbcT1 => Self::msbc_t1(),
            EscoMsbcT2 => Self::new(0x05, 0x000D, REDUCED_PACKET_MASK, 0x02, 32_000),
        }
    }

    /// HFP CVSD S1 settings (Profile v1.8 section 5.7).
    pub fn cvsd_s1() -> Self {
        Self::new(0x02, 0x0007, COMMON_PACKET_MASK, 0x01, 16_000)
    }

    /// HFP mSBC T1 settings (Profile v1.8 section 5.7).
    pub fn msbc_t1() -> Self {
        Self::new(0x05, 0x0008, COMMON_PACKET_MASK, 0x02, 32_000)
    }

    const fn new(
        codec_id: u8,
        max_latency: u16,
        packet_type: u16,
        retransmission_effort: u8,
        pcm_bandwidth: u32,
    ) -> Self {
        Self {
            transmit_coding_format: coding(codec_id),
            receive_coding_format: coding(codec_id),
            max_latency,
            packet_type,
            retransmission_effort,
            input_bandwidth: pcm_bandwidth,
            output_bandwidth: pcm_bandwidth,
        }
    }

    pub fn setup_command(self, acl_connection_handle: u16) -> Command {
        self.command(Some(acl_connection_handle), None)
    }

    pub fn accept_command(self, peer_address: Address) -> Command {
        self.command(None, Some(peer_address))
    }

    fn command(self, handle: Option<u16>, address: Option<Address>) -> Command {
        let common = CommonFields::from(self);
        match (handle, address) {
            (Some(connection_handle), None) => Command::EnhancedSetupSynchronousConnection {
                connection_handle,
                transmit_bandwidth: common.transmit_bandwidth,
                receive_bandwidth: common.receive_bandwidth,
                transmit_coding_format: common.transmit_coding_format,
                receive_coding_format: common.receive_coding_format,
                transmit_codec_frame_size: common.transmit_codec_frame_size,
                receive_codec_frame_size: common.receive_codec_frame_size,
                input_bandwidth: common.input_bandwidth,
                output_bandwidth: common.output_bandwidth,
                input_coding_format: common.input_coding_format,
                output_coding_format: common.output_coding_format,
                input_coded_data_size: common.input_coded_data_size,
                output_coded_data_size: common.output_coded_data_size,
                input_pcm_data_format: common.input_pcm_data_format,
                output_pcm_data_format: common.output_pcm_data_format,
                input_pcm_sample_payload_msb_position: 0,
                output_pcm_sample_payload_msb_position: 0,
                input_data_path: 0,
                output_data_path: 0,
                input_transport_unit_size: 0,
                output_transport_unit_size: 0,
                max_latency: common.max_latency,
                packet_type: common.packet_type,
                retransmission_effort: common.retransmission_effort,
            },
            (None, Some(bd_addr)) => Command::EnhancedAcceptSynchronousConnectionRequest {
                bd_addr,
                transmit_bandwidth: common.transmit_bandwidth,
                receive_bandwidth: common.receive_bandwidth,
                transmit_coding_format: common.transmit_coding_format,
                receive_coding_format: common.receive_coding_format,
                transmit_codec_frame_size: common.transmit_codec_frame_size,
                receive_codec_frame_size: common.receive_codec_frame_size,
                input_bandwidth: common.input_bandwidth,
                output_bandwidth: common.output_bandwidth,
                input_coding_format: common.input_coding_format,
                output_coding_format: common.output_coding_format,
                input_coded_data_size: common.input_coded_data_size,
                output_coded_data_size: common.output_coded_data_size,
                input_pcm_data_format: common.input_pcm_data_format,
                output_pcm_data_format: common.output_pcm_data_format,
                input_pcm_sample_payload_msb_position: 0,
                output_pcm_sample_payload_msb_position: 0,
                input_data_path: 0,
                output_data_path: 0,
                input_transport_unit_size: 0,
                output_transport_unit_size: 0,
                max_latency: common.max_latency,
                packet_type: common.packet_type,
                retransmission_effort: common.retransmission_effort,
            },
            _ => unreachable!("exactly one synchronous endpoint is required"),
        }
    }
}

pub fn parameters_for_codec(codec: AudioCodec) -> EscoParameters {
    match codec {
        AudioCodec::Msbc => EscoParameters::msbc_t1(),
        _ => EscoParameters::cvsd_s1(),
    }
}

#[derive(Clone, Copy)]
struct CommonFields {
    transmit_bandwidth: u32,
    receive_bandwidth: u32,
    transmit_coding_format: CodingFormat,
    receive_coding_format: CodingFormat,
    transmit_codec_frame_size: u16,
    receive_codec_frame_size: u16,
    input_bandwidth: u32,
    output_bandwidth: u32,
    input_coding_format: CodingFormat,
    output_coding_format: CodingFormat,
    input_coded_data_size: u16,
    output_coded_data_size: u16,
    input_pcm_data_format: u8,
    output_pcm_data_format: u8,
    max_latency: u16,
    packet_type: u16,
    retransmission_effort: u8,
}

impl From<EscoParameters> for CommonFields {
    fn from(value: EscoParameters) -> Self {
        Self {
            transmit_bandwidth: 8000,
            receive_bandwidth: 8000,
            transmit_coding_format: value.transmit_coding_format,
            receive_coding_format: value.receive_coding_format,
            transmit_codec_frame_size: 60,
            receive_codec_frame_size: 60,
            input_bandwidth: value.input_bandwidth,
            output_bandwidth: value.output_bandwidth,
            input_coding_format: coding(0x04),
            output_coding_format: coding(0x04),
            input_coded_data_size: 16,
            output_coded_data_size: 16,
            input_pcm_data_format: 0,
            output_pcm_data_format: 0,
            max_latency: value.max_latency,
            packet_type: value.packet_type,
            retransmission_effort: value.retransmission_effort,
        }
    }
}

const fn coding(coding_format: u8) -> CodingFormat {
    CodingFormat {
        coding_format,
        company_id: 0,
        vendor_specific_codec_id: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_upstream_default_parameter_sets_are_available() {
        use DefaultCodecParameters::*;

        let cases = [
            (ScoCvsdD0, 0x02, 0xFFFF, HV1, 0x00, 16_000),
            (ScoCvsdD1, 0x02, 0xFFFF, HV3, 0x00, 16_000),
            (EscoCvsdS1, 0x02, 0x0007, COMMON_PACKET_MASK, 0x01, 16_000),
            (EscoCvsdS2, 0x02, 0x0007, REDUCED_PACKET_MASK, 0x01, 16_000),
            (EscoCvsdS3, 0x02, 0x000A, REDUCED_PACKET_MASK, 0x01, 16_000),
            (EscoCvsdS4, 0x02, 0x000C, REDUCED_PACKET_MASK, 0x02, 16_000),
            (EscoMsbcT1, 0x05, 0x0008, COMMON_PACKET_MASK, 0x02, 32_000),
            (EscoMsbcT2, 0x05, 0x000D, REDUCED_PACKET_MASK, 0x02, 32_000),
        ];

        for (preset, codec, latency, packet_type, effort, bandwidth) in cases {
            let parameters = EscoParameters::for_default(preset);
            assert_eq!(parameters.transmit_coding_format.coding_format, codec);
            assert_eq!(parameters.receive_coding_format.coding_format, codec);
            assert_eq!(parameters.max_latency, latency);
            assert_eq!(parameters.packet_type, packet_type);
            assert_eq!(parameters.retransmission_effort, effort);
            assert_eq!(parameters.input_bandwidth, bandwidth);
            assert_eq!(parameters.output_bandwidth, bandwidth);
        }
    }

    #[test]
    fn negotiated_codec_selects_the_interoperability_baseline() {
        assert_eq!(
            parameters_for_codec(AudioCodec::Cvsd),
            EscoParameters::for_default(DefaultCodecParameters::EscoCvsdS1)
        );
        assert_eq!(
            parameters_for_codec(AudioCodec::Msbc),
            EscoParameters::for_default(DefaultCodecParameters::EscoMsbcT1)
        );
    }
}
