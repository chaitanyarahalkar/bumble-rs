//! GENERATED (tools/hcigen/gen_surface.py): the HCI command surface that
//! upstream `controller.py` implements, with each command's HCI response shape.
//! Used by the software controller to give every command a well-formed reply
//! that matches upstream's behavior, instead of a blanket "Unknown Command".
//!
//! - `StatusOnly`: config/set commands upstream accepts and returns
//!   Command Complete + status SUCCESS for (it stores state; the in-process sim
//!   has no state to store, so it simply acknowledges).
//! - `Data`: commands that return read data. The controller answers the ones it
//!   can model with real values (see `handle_command`); the rest are
//!   acknowledged SUCCESS without a synthesized payload (documented stub).
//! - `Status`: commands that start an operation and complete via a later event
//!   (Command Status). Functionally simulated where the in-process link allows
//!   (e.g. connect/disconnect); otherwise acknowledged with Command Status.

/// The HCI response shape a command produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resp {
    /// Command Complete with a status-only return parameter.
    StatusOnly,
    /// Command Complete carrying read data.
    Data,
    /// Command Status (an operation that completes via a later event).
    Status,
}

/// (op_code, response shape) for every command upstream `controller.py` handles.
pub static COMMAND_SURFACE: &[(u16, Resp)] = &[
    (0x0405, Resp::Status),     // create_connection
    (0x0406, Resp::Status),     // disconnect
    (0x0409, Resp::Status),     // accept_connection_request
    (0x0419, Resp::Status),     // remote_name_request
    (0x041B, Resp::Status),     // read_remote_supported_features
    (0x041C, Resp::Status),     // read_remote_extended_features
    (0x043D, Resp::Status),     // enhanced_setup_synchronous_connection
    (0x043E, Resp::Status),     // enhanced_accept_synchronous_connection_request
    (0x0803, Resp::Status),     // sniff_mode
    (0x0804, Resp::Status),     // exit_sniff_mode
    (0x080B, Resp::Status),     // switch_role
    (0x0C01, Resp::StatusOnly), // set_event_mask
    (0x0C03, Resp::StatusOnly), // reset
    (0x0C13, Resp::StatusOnly), // write_local_name
    (0x0C14, Resp::Data),       // read_local_name
    (0x0C1A, Resp::StatusOnly), // write_scan_enable
    (0x0C23, Resp::Data),       // read_class_of_device
    (0x0C24, Resp::StatusOnly), // write_class_of_device
    (0x0C2E, Resp::Data),       // read_synchronous_flow_control_enable
    (0x0C2F, Resp::StatusOnly), // write_synchronous_flow_control_enable
    (0x0C31, Resp::StatusOnly), // set_controller_to_host_flow_control
    (0x0C33, Resp::StatusOnly), // host_buffer_size
    (0x0C52, Resp::StatusOnly), // write_extended_inquiry_response
    (0x0C56, Resp::StatusOnly), // write_simple_pairing_mode
    (0x0C63, Resp::StatusOnly), // set_event_mask_page_2
    (0x0C6C, Resp::Data),       // read_le_host_support
    (0x0C6D, Resp::StatusOnly), // write_le_host_support
    (0x0C7C, Resp::Data),       // write_authenticated_payload_timeout
    (0x1001, Resp::Data),       // read_local_version_information
    (0x1002, Resp::Data),       // read_local_supported_commands
    (0x1003, Resp::Data),       // read_local_supported_features
    (0x1004, Resp::Data),       // read_local_extended_features
    (0x1005, Resp::Data),       // read_buffer_size
    (0x1009, Resp::Data),       // read_bd_addr
    (0x2001, Resp::StatusOnly), // le_set_event_mask
    (0x2002, Resp::Data),       // le_read_buffer_size
    (0x2003, Resp::Data),       // le_read_local_supported_features
    (0x2005, Resp::StatusOnly), // le_set_random_address
    (0x2006, Resp::StatusOnly), // le_set_advertising_parameters
    (0x2007, Resp::Data),       // le_read_advertising_physical_channel_tx_power
    (0x2008, Resp::StatusOnly), // le_set_advertising_data
    (0x2009, Resp::StatusOnly), // le_set_scan_response_data
    (0x200A, Resp::StatusOnly), // le_set_advertising_enable
    (0x200B, Resp::StatusOnly), // le_set_scan_parameters
    (0x200C, Resp::StatusOnly), // le_set_scan_enable
    (0x200D, Resp::Status),     // le_create_connection
    (0x200E, Resp::StatusOnly), // le_create_connection_cancel
    (0x200F, Resp::Data),       // le_read_filter_accept_list_size
    (0x2010, Resp::StatusOnly), // le_clear_filter_accept_list
    (0x2011, Resp::StatusOnly), // le_add_device_to_filter_accept_list
    (0x2012, Resp::StatusOnly), // le_remove_device_from_filter_accept_list
    (0x2016, Resp::Status),     // le_read_remote_features
    (0x2018, Resp::Data),       // le_rand
    (0x2019, Resp::Status),     // le_enable_encryption
    (0x201C, Resp::Data),       // le_read_supported_states
    (0x2023, Resp::Data),       // le_read_suggested_default_data_length
    (0x2024, Resp::StatusOnly), // le_write_suggested_default_data_length
    (0x2025, Resp::StatusOnly), // le_read_local_p_256_public_key
    (0x2027, Resp::StatusOnly), // le_add_device_to_resolving_list
    (0x2029, Resp::StatusOnly), // le_clear_resolving_list
    (0x202A, Resp::Data),       // le_read_resolving_list_size
    (0x202D, Resp::StatusOnly), // le_set_address_resolution_enable
    (0x202E, Resp::StatusOnly), // le_set_resolvable_private_address_timeout
    (0x202F, Resp::Data),       // le_read_maximum_data_length
    (0x2030, Resp::Data),       // le_read_phy
    (0x2031, Resp::StatusOnly), // le_set_default_phy
    (0x2035, Resp::StatusOnly), // le_set_advertising_set_random_address
    (0x2036, Resp::Data),       // le_set_extended_advertising_parameters
    (0x2037, Resp::StatusOnly), // le_set_extended_advertising_data
    (0x2038, Resp::StatusOnly), // le_set_extended_scan_response_data
    (0x2039, Resp::StatusOnly), // le_set_extended_advertising_enable
    (0x203A, Resp::Data),       // le_read_maximum_advertising_data_length
    (0x203B, Resp::Data),       // le_read_number_of_supported_advertising_sets
    (0x203C, Resp::StatusOnly), // le_remove_advertising_set
    (0x203D, Resp::StatusOnly), // le_clear_advertising_sets
    (0x203E, Resp::StatusOnly), // le_set_periodic_advertising_parameters
    (0x203F, Resp::StatusOnly), // le_set_periodic_advertising_data
    (0x2040, Resp::StatusOnly), // le_set_periodic_advertising_enable
    (0x2043, Resp::Status),     // le_extended_create_connection
    (0x204B, Resp::Data),       // le_read_transmit_power
    (0x2060, Resp::Data),       // le_read_buffer_size_v2
    (0x2062, Resp::Data),       // le_set_cig_parameters
    (0x2064, Resp::Status),     // le_create_cis
    (0x2065, Resp::Data),       // le_remove_cig
    (0x2066, Resp::Status),     // le_accept_cis_request
    (0x206E, Resp::Data),       // le_setup_iso_data_path
    (0x206F, Resp::Data),       // le_remove_iso_data_path
    (0x2074, Resp::StatusOnly), // le_set_host_feature
    (0x207D, Resp::StatusOnly), // le_set_default_subrate
    (0x207E, Resp::Status),     // le_subrate_request
    (0x2087, Resp::Data),       // le_read_all_local_supported_features
    (0x2041, Resp::StatusOnly), // le_set_extended_scan_parameters
    (0x2043, Resp::Status),     // le_extended_create_connection
];

/// The response shape upstream's controller uses for `op_code`, if it handles it.
pub fn response_kind(op_code: u16) -> Option<Resp> {
    COMMAND_SURFACE
        .iter()
        .find(|(o, _)| *o == op_code)
        .map(|(_, r)| *r)
}
