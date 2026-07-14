//! Pandora Bluetooth conformance services backed by bumble-rs.

// Tonic fixes the service error type to `tonic::Status`; boxing it would break
// the generated Pandora service contracts.
#![allow(clippy::result_large_err)]

pub mod config;
mod data_types;
mod host;
mod l2cap;
mod runtime;

// Keep the canonical bt-test-interfaces v0.0.6 documentation verbatim in the
// generated API, including its deliberately nested list indentation.
#[allow(
    clippy::derive_partial_eq_without_eq,
    clippy::doc_overindented_list_items,
    rustdoc::invalid_html_tags
)]
pub mod proto {
    tonic::include_proto!("pandora");

    pub mod l2cap {
        tonic::include_proto!("pandora.l2cap");
    }
}

pub use config::{PandoraConfig, ServerConfig};
pub use host::HostService;
pub use l2cap::L2capService;
pub use runtime::PandoraRuntime;
