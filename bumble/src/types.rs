//! Small shared core enums, ported from `bumble.core`.

/// The physical transport of a connection (Vol 1, Part A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalTransport(pub u8);

impl PhysicalTransport {
    /// BR/EDR (Classic).
    pub const BR_EDR: PhysicalTransport = PhysicalTransport(0);
    /// LE.
    pub const LE: PhysicalTransport = PhysicalTransport(1);
}

/// The LE role advertised in the LE Role AD structure (Core Spec Supplement,
/// Part A - 1.17). Open enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeRole(pub u8);

impl LeRole {
    pub const PERIPHERAL_ONLY: LeRole = LeRole(0x00);
    pub const CENTRAL_ONLY: LeRole = LeRole(0x01);
    pub const PERIPHERAL_PREFERRED: LeRole = LeRole(0x02);
    pub const CENTRAL_PREFERRED: LeRole = LeRole(0x03);
}
