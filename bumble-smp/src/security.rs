//! Security Request evaluation against persisted LE bonds.

use bumble::keys::{Key, PairingKeys};

use crate::{AuthReq, PairingRole, SmpPdu};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BondEncryption {
    pub long_term_key: [u8; 16],
    pub encrypted_diversifier: u16,
    pub random_number: [u8; 8],
    pub authenticated: bool,
    pub secure_connections: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecurityRequestAction {
    EnableEncryption(BondEncryption),
    Pair,
}

/// Build the peripheral-to-central SMP Security Request command.
pub fn security_request(auth_req: AuthReq) -> SmpPdu {
    SmpPdu::SecurityRequest {
        auth_req: auth_req.0,
    }
}

/// Decide whether a stored bond satisfies a peer's Security Request.
///
/// SC bonds use the shared `ltk`. Legacy bonds select the central/peripheral
/// directional LTK according to the local connection role. Missing, malformed,
/// unauthenticated, or non-SC material falls back to a fresh pairing procedure.
pub fn security_request_action(
    requested: AuthReq,
    local_role: PairingRole,
    bond: Option<&PairingKeys>,
) -> SecurityRequestAction {
    let Some(bond) = bond else {
        return SecurityRequestAction::Pair;
    };
    let (key, secure_connections) = if let Some(key) = bond.ltk.as_ref() {
        (key, true)
    } else {
        let directional = match local_role {
            PairingRole::Initiator => bond.ltk_central.as_ref(),
            PairingRole::Responder => bond.ltk_peripheral.as_ref(),
        };
        let Some(key) = directional else {
            return SecurityRequestAction::Pair;
        };
        (key, false)
    };

    if requested.contains(AuthReq::SECURE_CONNECTIONS) && !secure_connections {
        return SecurityRequestAction::Pair;
    }
    if requested.contains(AuthReq::MITM) && !key.authenticated {
        return SecurityRequestAction::Pair;
    }
    decode_key(key, secure_connections).map_or(SecurityRequestAction::Pair, |encryption| {
        SecurityRequestAction::EnableEncryption(encryption)
    })
}

fn decode_key(key: &Key, secure_connections: bool) -> Option<BondEncryption> {
    let long_term_key = key.value.as_slice().try_into().ok()?;
    let random_number = match key.rand.as_deref() {
        Some(rand) => rand.try_into().ok()?,
        None => [0; 8],
    };
    Some(BondEncryption {
        long_term_key,
        encrypted_diversifier: key.ediv.unwrap_or(0),
        random_number,
        authenticated: key.authenticated,
        secure_connections,
    })
}
