//! Classic (BR/EDR) Link Manager Protocol PDUs exchanged between controllers
//! over the [`LocalLink`](crate::LocalLink). [`Opcode`] and [`Packet`] provide
//! the complete serialized `bumble.lmp` catalog; [`ClassicPdu`] is the compact
//! semantic form used by the deterministic in-process controller link. Together
//! they preserve both the byte contract and host-visible state transitions,
//! including role switching during and after Classic connection establishment.

use std::fmt;

/// Open LMP opcode value, including one-byte base and two-byte escape opcodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Opcode(pub u16);

impl Opcode {
    pub const LMP_ACCEPTED: Self = Self(0x0003);
    pub const LMP_ACCEPTED_EXT: Self = Self(0x7F01);
    pub const LMP_AU_RAND: Self = Self(0x000B);
    pub const LMP_AUTO_RATE: Self = Self(0x0023);
    pub const LMP_CHANNEL_CLASSIFICATION: Self = Self(0x7F11);
    pub const LMP_CHANNEL_CLASSIFICATION_REQ: Self = Self(0x7F10);
    pub const LMP_CLK_ADJ: Self = Self(0x7F05);
    pub const LMP_CLK_ADJ_ACK: Self = Self(0x7F06);
    pub const LMP_CLK_ADJ_REQ: Self = Self(0x7F07);
    pub const LMP_CLKOFFSET_REQ: Self = Self(0x0005);
    pub const LMP_CLKOFFSET_RES: Self = Self(0x0006);
    pub const LMP_COMB_KEY: Self = Self(0x0009);
    pub const LMP_DECR_POWER_REQ: Self = Self(0x0020);
    pub const LMP_DETACH: Self = Self(0x0007);
    pub const LMP_DHKEY_CHECK: Self = Self(0x0041);
    pub const LMP_ENCAPSULATED_HEADER: Self = Self(0x003D);
    pub const LMP_ENCAPSULATED_PAYLOAD: Self = Self(0x003E);
    pub const LMP_ENCRYPTION_KEY_SIZE_MASK_REQ: Self = Self(0x003A);
    pub const LMP_ENCRYPTION_KEY_SIZE_MASK_RES: Self = Self(0x003B);
    pub const LMP_ENCRYPTION_KEY_SIZE_REQ: Self = Self(0x0010);
    pub const LMP_ENCRYPTION_MODE_REQ: Self = Self(0x000F);
    pub const LMP_ESCO_LINK_REQ: Self = Self(0x7F0C);
    pub const LMP_FEATURES_REQ: Self = Self(0x0027);
    pub const LMP_FEATURES_REQ_EXT: Self = Self(0x7F03);
    pub const LMP_FEATURES_RES: Self = Self(0x0028);
    pub const LMP_FEATURES_RES_EXT: Self = Self(0x7F04);
    pub const LMP_HOLD: Self = Self(0x0014);
    pub const LMP_HOLD_REQ: Self = Self(0x0015);
    pub const LMP_HOST_CONNECTION_REQ: Self = Self(0x0033);
    pub const LMP_IN_RAND: Self = Self(0x0008);
    pub const LMP_INCR_POWER_REQ: Self = Self(0x001F);
    pub const LMP_IO_CAPABILITY_REQ: Self = Self(0x7F19);
    pub const LMP_IO_CAPABILITY_RES: Self = Self(0x7F1A);
    pub const LMP_KEYPRESS_NOTIFICATION: Self = Self(0x7F1E);
    pub const LMP_MAX_POWER: Self = Self(0x0021);
    pub const LMP_MAX_SLOT: Self = Self(0x002D);
    pub const LMP_MAX_SLOT_REQ: Self = Self(0x002E);
    pub const LMP_MIN_POWER: Self = Self(0x0022);
    pub const LMP_NAME_REQ: Self = Self(0x0001);
    pub const LMP_NAME_RES: Self = Self(0x0002);
    pub const LMP_NOT_ACCEPTED: Self = Self(0x0004);
    pub const LMP_NOT_ACCEPTED_EXT: Self = Self(0x7F02);
    pub const LMP_NUMERIC_COMPARISON_FAILED: Self = Self(0x7F1B);
    pub const LMP_OOB_FAILED: Self = Self(0x7F1D);
    pub const LMP_PACKET_TYPE_TABLE_REQ: Self = Self(0x7F0B);
    pub const LMP_PAGE_MODE_REQ: Self = Self(0x0035);
    pub const LMP_PAGE_SCAN_MODE_REQ: Self = Self(0x0036);
    pub const LMP_PASSKEY_FAILED: Self = Self(0x7F1C);
    pub const LMP_PAUSE_ENCRYPTION_AES_REQ: Self = Self(0x0042);
    pub const LMP_PAUSE_ENCRYPTION_REQ: Self = Self(0x7F17);
    pub const LMP_PING_REQ: Self = Self(0x7F21);
    pub const LMP_PING_RES: Self = Self(0x7F22);
    pub const LMP_POWER_CONTROL_REQ: Self = Self(0x7F1F);
    pub const LMP_POWER_CONTROL_RES: Self = Self(0x7F20);
    pub const LMP_PREFERRED_RATE: Self = Self(0x0024);
    pub const LMP_QUALITY_OF_SERVICE: Self = Self(0x0029);
    pub const LMP_QUALITY_OF_SERVICE_REQ: Self = Self(0x002A);
    pub const LMP_REMOVE_ESCO_LINK_REQ: Self = Self(0x7F0D);
    pub const LMP_REMOVE_SCO_LINK_REQ: Self = Self(0x002C);
    pub const LMP_RESUME_ENCRYPTION_REQ: Self = Self(0x7F18);
    pub const LMP_SAM_DEFINE_MAP: Self = Self(0x7F24);
    pub const LMP_SAM_SET_TYPE0: Self = Self(0x7F23);
    pub const LMP_SAM_SWITCH: Self = Self(0x7F25);
    pub const LMP_SCO_LINK_REQ: Self = Self(0x002B);
    pub const LMP_SET_AFH: Self = Self(0x003C);
    pub const LMP_SETUP_COMPLETE: Self = Self(0x0031);
    pub const LMP_SIMPLE_PAIRING_CONFIRM: Self = Self(0x003F);
    pub const LMP_SIMPLE_PAIRING_NUMBER: Self = Self(0x0040);
    pub const LMP_SLOT_OFFSET: Self = Self(0x0034);
    pub const LMP_SNIFF_REQ: Self = Self(0x0017);
    pub const LMP_SNIFF_SUBRATING_REQ: Self = Self(0x7F15);
    pub const LMP_SNIFF_SUBRATING_RES: Self = Self(0x7F16);
    pub const LMP_SRES: Self = Self(0x000C);
    pub const LMP_START_ENCRYPTION_REQ: Self = Self(0x0011);
    pub const LMP_STOP_ENCRYPTION_REQ: Self = Self(0x0012);
    pub const LMP_SUPERVISION_TIMEOUT: Self = Self(0x0037);
    pub const LMP_SWITCH_REQ: Self = Self(0x0013);
    pub const LMP_TEMP_KEY: Self = Self(0x000E);
    pub const LMP_TEMP_RAND: Self = Self(0x000D);
    pub const LMP_TEST_ACTIVATE: Self = Self(0x0038);
    pub const LMP_TEST_CONTROL: Self = Self(0x0039);
    pub const LMP_TIMING_ACCURACY_REQ: Self = Self(0x002F);
    pub const LMP_TIMING_ACCURACY_RES: Self = Self(0x0030);
    pub const LMP_UNIT_KEY: Self = Self(0x000A);
    pub const LMP_UNSNIFF_REQ: Self = Self(0x0018);
    pub const LMP_USE_SEMI_PERMANENT_KEY: Self = Self(0x0032);
    pub const LMP_VERSION_REQ: Self = Self(0x0025);
    pub const LMP_VERSION_RES: Self = Self(0x0026);

    /// Numeric opcode value.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Encode the base opcode as one byte or an escape opcode as big-endian.
    pub fn to_bytes(self) -> Vec<u8> {
        if self.0 > u16::from(u8::MAX) {
            self.0.to_be_bytes().to_vec()
        } else {
            vec![self.0 as u8]
        }
    }

    /// Parse an open opcode starting at `offset`, preserving unknown values.
    pub fn parse_from(data: &[u8], offset: usize) -> Result<(usize, Self), CodecError> {
        let first = *data.get(offset).ok_or(CodecError::Truncated {
            expected_at_least: offset + 1,
            actual: data.len(),
        })?;
        if matches!(first, 124 | 127) {
            let second = *data.get(offset + 1).ok_or(CodecError::Truncated {
                expected_at_least: offset + 2,
                actual: data.len(),
            })?;
            Ok((offset + 2, Self(u16::from_be_bytes([first, second]))))
        } else {
            Ok((offset + 1, Self(u16::from(first))))
        }
    }
}

/// LMP packet codec error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    Truncated {
        expected_at_least: usize,
        actual: usize,
    },
    InvalidLength {
        opcode: Opcode,
        expected: usize,
        actual: usize,
    },
    NameLengthOutOfRange(u32),
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                expected_at_least,
                actual,
            } => write!(
                formatter,
                "truncated LMP packet: need at least {expected_at_least} bytes, got {actual}"
            ),
            Self::InvalidLength {
                opcode,
                expected,
                actual,
            } => write!(
                formatter,
                "invalid LMP payload length for 0x{:04X}: expected {expected}, got {actual}",
                opcode.0
            ),
            Self::NameLengthOutOfRange(length) => {
                write!(formatter, "LMP name length {length} exceeds 24 bits")
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Serialized LMP packet catalog registered by upstream `lmp.py`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    Accepted {
        response_opcode: Opcode,
    },
    NotAccepted {
        response_opcode: Opcode,
        error_code: u8,
    },
    AcceptedExt {
        response_opcode: Opcode,
    },
    NotAcceptedExt {
        response_opcode: Opcode,
        error_code: u8,
    },
    AuRand {
        random_number: [u8; 16],
    },
    Detach {
        error_code: u8,
    },
    EscoLinkReq {
        esco_handle: u8,
        esco_lt_addr: u8,
        timing_control_flags: u8,
        d_esco: u8,
        t_esco: u8,
        w_esco: u8,
        esco_packet_type_c_to_p: u8,
        esco_packet_type_p_to_c: u8,
        packet_length_c_to_p: u16,
        packet_length_p_to_c: u16,
        air_mode: u8,
        negotiation_state: u8,
    },
    HostConnectionReq,
    RemoveEscoLinkReq {
        esco_handle: u8,
        error_code: u8,
    },
    RemoveScoLinkReq {
        sco_handle: u8,
        error_code: u8,
    },
    ScoLinkReq {
        sco_handle: u8,
        timing_control_flags: u8,
        d_sco: u8,
        t_sco: u8,
        sco_packet: u8,
        air_mode: u8,
    },
    SwitchReq {
        switch_instant: u32,
    },
    NameReq {
        name_offset: u16,
    },
    NameRes {
        name_offset: u16,
        name_length: u32,
        name_fragment: Vec<u8>,
    },
    FeaturesReq {
        features: [u8; 8],
    },
    FeaturesRes {
        features: [u8; 8],
    },
    FeaturesReqExt {
        features_page: u8,
        features: [u8; 8],
    },
    FeaturesResExt {
        features_page: u8,
        max_features_page: u8,
        features: [u8; 8],
    },
    Unknown {
        opcode: Opcode,
        payload: Vec<u8>,
    },
}

impl Packet {
    /// Packet opcode.
    pub const fn opcode(&self) -> Opcode {
        match self {
            Self::Accepted { .. } => Opcode::LMP_ACCEPTED,
            Self::NotAccepted { .. } => Opcode::LMP_NOT_ACCEPTED,
            Self::AcceptedExt { .. } => Opcode::LMP_ACCEPTED_EXT,
            Self::NotAcceptedExt { .. } => Opcode::LMP_NOT_ACCEPTED_EXT,
            Self::AuRand { .. } => Opcode::LMP_AU_RAND,
            Self::Detach { .. } => Opcode::LMP_DETACH,
            Self::EscoLinkReq { .. } => Opcode::LMP_ESCO_LINK_REQ,
            Self::HostConnectionReq => Opcode::LMP_HOST_CONNECTION_REQ,
            Self::RemoveEscoLinkReq { .. } => Opcode::LMP_REMOVE_ESCO_LINK_REQ,
            Self::RemoveScoLinkReq { .. } => Opcode::LMP_REMOVE_SCO_LINK_REQ,
            Self::ScoLinkReq { .. } => Opcode::LMP_SCO_LINK_REQ,
            Self::SwitchReq { .. } => Opcode::LMP_SWITCH_REQ,
            Self::NameReq { .. } => Opcode::LMP_NAME_REQ,
            Self::NameRes { .. } => Opcode::LMP_NAME_RES,
            Self::FeaturesReq { .. } => Opcode::LMP_FEATURES_REQ,
            Self::FeaturesRes { .. } => Opcode::LMP_FEATURES_RES,
            Self::FeaturesReqExt { .. } => Opcode::LMP_FEATURES_REQ_EXT,
            Self::FeaturesResExt { .. } => Opcode::LMP_FEATURES_RES_EXT,
            Self::Unknown { opcode, .. } => *opcode,
        }
    }

    /// Decode one complete LMP packet, preserving unregistered opcodes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, CodecError> {
        let (offset, opcode) = Opcode::parse_from(data, 0)?;
        let payload = &data[offset..];
        let packet = match opcode {
            Opcode::LMP_ACCEPTED => Self::Accepted {
                response_opcode: parse_response_opcode(opcode, payload, false)?.0,
            },
            Opcode::LMP_NOT_ACCEPTED => {
                let (response_opcode, used) = parse_response_opcode(opcode, payload, true)?;
                Self::NotAccepted {
                    response_opcode,
                    error_code: payload[used],
                }
            }
            Opcode::LMP_ACCEPTED_EXT => Self::AcceptedExt {
                response_opcode: parse_response_opcode(opcode, payload, false)?.0,
            },
            Opcode::LMP_NOT_ACCEPTED_EXT => {
                let (response_opcode, used) = parse_response_opcode(opcode, payload, true)?;
                Self::NotAcceptedExt {
                    response_opcode,
                    error_code: payload[used],
                }
            }
            Opcode::LMP_AU_RAND => Self::AuRand {
                random_number: fixed_array(opcode, payload)?,
            },
            Opcode::LMP_DETACH => {
                expect_length(opcode, payload, 1)?;
                Self::Detach {
                    error_code: payload[0],
                }
            }
            Opcode::LMP_ESCO_LINK_REQ => {
                expect_length(opcode, payload, 14)?;
                Self::EscoLinkReq {
                    esco_handle: payload[0],
                    esco_lt_addr: payload[1],
                    timing_control_flags: payload[2],
                    d_esco: payload[3],
                    t_esco: payload[4],
                    w_esco: payload[5],
                    esco_packet_type_c_to_p: payload[6],
                    esco_packet_type_p_to_c: payload[7],
                    packet_length_c_to_p: u16::from_le_bytes([payload[8], payload[9]]),
                    packet_length_p_to_c: u16::from_le_bytes([payload[10], payload[11]]),
                    air_mode: payload[12],
                    negotiation_state: payload[13],
                }
            }
            Opcode::LMP_HOST_CONNECTION_REQ => {
                expect_length(opcode, payload, 0)?;
                Self::HostConnectionReq
            }
            Opcode::LMP_REMOVE_ESCO_LINK_REQ => {
                expect_length(opcode, payload, 2)?;
                Self::RemoveEscoLinkReq {
                    esco_handle: payload[0],
                    error_code: payload[1],
                }
            }
            Opcode::LMP_REMOVE_SCO_LINK_REQ => {
                expect_length(opcode, payload, 2)?;
                Self::RemoveScoLinkReq {
                    sco_handle: payload[0],
                    error_code: payload[1],
                }
            }
            Opcode::LMP_SCO_LINK_REQ => {
                expect_length(opcode, payload, 6)?;
                Self::ScoLinkReq {
                    sco_handle: payload[0],
                    timing_control_flags: payload[1],
                    d_sco: payload[2],
                    t_sco: payload[3],
                    sco_packet: payload[4],
                    air_mode: payload[5],
                }
            }
            Opcode::LMP_SWITCH_REQ => {
                expect_length(opcode, payload, 4)?;
                Self::SwitchReq {
                    switch_instant: u32::from_le_bytes(payload.try_into().expect("length checked")),
                }
            }
            Opcode::LMP_NAME_REQ => {
                expect_length(opcode, payload, 2)?;
                Self::NameReq {
                    name_offset: u16::from_le_bytes([payload[0], payload[1]]),
                }
            }
            Opcode::LMP_NAME_RES => {
                if payload.len() < 5 {
                    return Err(CodecError::Truncated {
                        expected_at_least: offset + 5,
                        actual: data.len(),
                    });
                }
                Self::NameRes {
                    name_offset: u16::from_le_bytes([payload[0], payload[1]]),
                    name_length: u32::from(payload[2])
                        | (u32::from(payload[3]) << 8)
                        | (u32::from(payload[4]) << 16),
                    name_fragment: payload[5..].to_vec(),
                }
            }
            Opcode::LMP_FEATURES_REQ => Self::FeaturesReq {
                features: fixed_array(opcode, payload)?,
            },
            Opcode::LMP_FEATURES_RES => Self::FeaturesRes {
                features: fixed_array(opcode, payload)?,
            },
            Opcode::LMP_FEATURES_REQ_EXT => {
                expect_length(opcode, payload, 9)?;
                Self::FeaturesReqExt {
                    features_page: payload[0],
                    features: payload[1..].try_into().expect("length checked"),
                }
            }
            Opcode::LMP_FEATURES_RES_EXT => {
                expect_length(opcode, payload, 10)?;
                Self::FeaturesResExt {
                    features_page: payload[0],
                    max_features_page: payload[1],
                    features: payload[2..].try_into().expect("length checked"),
                }
            }
            _ => Self::Unknown {
                opcode,
                payload: payload.to_vec(),
            },
        };
        Ok(packet)
    }

    /// Encode one LMP packet using upstream's little-endian field layout.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
        let mut bytes = self.opcode().to_bytes();
        match self {
            Self::Accepted { response_opcode } | Self::AcceptedExt { response_opcode } => {
                bytes.extend(response_opcode.to_bytes());
            }
            Self::NotAccepted {
                response_opcode,
                error_code,
            }
            | Self::NotAcceptedExt {
                response_opcode,
                error_code,
            } => {
                bytes.extend(response_opcode.to_bytes());
                bytes.push(*error_code);
            }
            Self::AuRand { random_number } => bytes.extend_from_slice(random_number),
            Self::Detach { error_code } => bytes.push(*error_code),
            Self::EscoLinkReq {
                esco_handle,
                esco_lt_addr,
                timing_control_flags,
                d_esco,
                t_esco,
                w_esco,
                esco_packet_type_c_to_p,
                esco_packet_type_p_to_c,
                packet_length_c_to_p,
                packet_length_p_to_c,
                air_mode,
                negotiation_state,
            } => {
                bytes.extend_from_slice(&[
                    *esco_handle,
                    *esco_lt_addr,
                    *timing_control_flags,
                    *d_esco,
                    *t_esco,
                    *w_esco,
                    *esco_packet_type_c_to_p,
                    *esco_packet_type_p_to_c,
                ]);
                bytes.extend_from_slice(&packet_length_c_to_p.to_le_bytes());
                bytes.extend_from_slice(&packet_length_p_to_c.to_le_bytes());
                bytes.extend_from_slice(&[*air_mode, *negotiation_state]);
            }
            Self::HostConnectionReq => {}
            Self::RemoveEscoLinkReq {
                esco_handle,
                error_code,
            } => bytes.extend_from_slice(&[*esco_handle, *error_code]),
            Self::RemoveScoLinkReq {
                sco_handle,
                error_code,
            } => bytes.extend_from_slice(&[*sco_handle, *error_code]),
            Self::ScoLinkReq {
                sco_handle,
                timing_control_flags,
                d_sco,
                t_sco,
                sco_packet,
                air_mode,
            } => bytes.extend_from_slice(&[
                *sco_handle,
                *timing_control_flags,
                *d_sco,
                *t_sco,
                *sco_packet,
                *air_mode,
            ]),
            Self::SwitchReq { switch_instant } => {
                bytes.extend_from_slice(&switch_instant.to_le_bytes());
            }
            Self::NameReq { name_offset } => {
                bytes.extend_from_slice(&name_offset.to_le_bytes());
            }
            Self::NameRes {
                name_offset,
                name_length,
                name_fragment,
            } => {
                if *name_length > 0x00FF_FFFF {
                    return Err(CodecError::NameLengthOutOfRange(*name_length));
                }
                bytes.extend_from_slice(&name_offset.to_le_bytes());
                bytes.extend_from_slice(&[
                    *name_length as u8,
                    (*name_length >> 8) as u8,
                    (*name_length >> 16) as u8,
                ]);
                bytes.extend_from_slice(name_fragment);
            }
            Self::FeaturesReq { features } | Self::FeaturesRes { features } => {
                bytes.extend_from_slice(features);
            }
            Self::FeaturesReqExt {
                features_page,
                features,
            } => {
                bytes.push(*features_page);
                bytes.extend_from_slice(features);
            }
            Self::FeaturesResExt {
                features_page,
                max_features_page,
                features,
            } => {
                bytes.extend_from_slice(&[*features_page, *max_features_page]);
                bytes.extend_from_slice(features);
            }
            Self::Unknown { payload, .. } => bytes.extend_from_slice(payload),
        }
        Ok(bytes)
    }
}

fn expect_length(opcode: Opcode, payload: &[u8], expected: usize) -> Result<(), CodecError> {
    if payload.len() == expected {
        Ok(())
    } else {
        Err(CodecError::InvalidLength {
            opcode,
            expected,
            actual: payload.len(),
        })
    }
}

fn fixed_array<const N: usize>(opcode: Opcode, payload: &[u8]) -> Result<[u8; N], CodecError> {
    expect_length(opcode, payload, N)?;
    Ok(payload.try_into().expect("length checked"))
}

fn parse_response_opcode(
    opcode: Opcode,
    payload: &[u8],
    has_error_code: bool,
) -> Result<(Opcode, usize), CodecError> {
    let (used, response_opcode) = Opcode::parse_from(payload, 0)?;
    expect_length(opcode, payload, used + usize::from(has_error_code))?;
    Ok((response_opcode, used))
}

/// A classic LMP PDU (the subset modelled by the software controller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassicPdu {
    /// Host-initiated connection request (`LmpHostConnectionReq`).
    HostConnectionReq,
    /// Connection accepted (`LmpAccepted` for `LMP_HOST_CONNECTION_REQ`).
    Accepted,
    /// Connection rejected (`LmpNotAccepted` for `LMP_HOST_CONNECTION_REQ`).
    Rejected { reason: u8 },
    /// Request that the two controllers exchange their Central/Peripheral roles.
    SwitchReq,
    /// Accept a pending role-switch request.
    SwitchAccepted,
    /// Reject a pending role-switch request.
    SwitchRejected { reason: u8 },
    /// Remote-name request (`LmpNameReq`).
    NameReq,
    /// Remote-name response (`LmpNameRes`); carries the 248-byte name field.
    NameRes { name: Vec<u8> },
    /// Features request (`LmpFeaturesReq`).
    FeaturesReq,
    /// Features response (`LmpFeaturesRes`).
    FeaturesRes { features: [u8; 8] },
    /// Extended-features request (`LmpFeaturesReqExt`).
    FeaturesReqExt { page_number: u8, features: [u8; 8] },
    /// Extended-features response (`LmpFeaturesResExt`).
    FeaturesResExt {
        page_number: u8,
        max_page_number: u8,
        features: [u8; 8],
    },
    /// Enable or disable encryption on an established Classic ACL.
    EncryptionModeReq { enable: bool },
    /// Request an SCO/eSCO logical link over an established Classic ACL.
    SynchronousConnectionReq { link_type: u8, air_mode: u8 },
    /// Accept a pending SCO/eSCO logical link.
    SynchronousConnectionAccepted { link_type: u8, air_mode: u8 },
    /// Reject a pending SCO/eSCO logical link.
    SynchronousConnectionRejected { reason: u8 },
    /// Disconnect an established SCO/eSCO logical link without dropping ACL.
    SynchronousDetach { error_code: u8 },
    /// Detach / disconnect (`LmpDetach`).
    Detach { error_code: u8 },
}
