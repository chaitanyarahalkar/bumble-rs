//! Well-known 16-bit Bluetooth UUIDs (assigned numbers), ported from the
//! named `UUID` constants in `bumble.core`.
//!
//! A sorted `(uuid16, name)` table with a binary-search lookup.

/// Assigned 16-bit UUIDs with their names, sorted by value.
pub static WELL_KNOWN_UUIDS: &[(u16, &str)] = &[
    (0x0001, "SDP"),
    (0x0002, "UDP"),
    (0x0003, "RFCOMM"),
    (0x0004, "TCP"),
    (0x0005, "TCP-BIN"),
    (0x0006, "TCS-AT"),
    (0x0007, "ATT"),
    (0x0008, "OBEX"),
    (0x0009, "IP"),
    (0x000A, "FTP"),
    (0x000C, "HTTP"),
    (0x000E, "WSP"),
    (0x000F, "BNEP"),
    (0x0010, "UPNP"),
    (0x0011, "HIDP"),
    (0x0012, "HardcopyControlChannel"),
    (0x0014, "HardcopyDataChannel"),
    (0x0016, "HardcopyNotification"),
    (0x0017, "AVCTP"),
    (0x0019, "AVDTP"),
    (0x001B, "CMTP"),
    (0x001E, "MCAPControlChannel"),
    (0x001F, "MCAPDataChannel"),
    (0x0100, "L2CAP"),
    (0x1000, "ServiceDiscoveryServerServiceClassID"),
    (0x1001, "BrowseGroupDescriptorServiceClassID"),
    (0x1101, "SerialPort"),
    (0x1102, "LANAccessUsingPPP"),
    (0x1103, "DialupNetworking"),
    (0x1104, "IrMCSync"),
    (0x1105, "OBEXObjectPush"),
    (0x1106, "OBEXFileTransfer"),
    (0x1107, "IrMCSyncCommand"),
    (0x1108, "Headset"),
    (0x1109, "CordlessTelephony"),
    (0x110A, "AudioSource"),
    (0x110B, "AudioSink"),
    (0x110C, "A/V_RemoteControlTarget"),
    (0x110D, "AdvancedAudioDistribution"),
    (0x110E, "A/V_RemoteControl"),
    (0x110F, "A/V_RemoteControlController"),
    (0x1110, "Intercom"),
    (0x1111, "Fax"),
    (0x1112, "Headset - Audio Gateway"),
    (0x1113, "WAP"),
    (0x1114, "WAP_CLIENT"),
    (0x1115, "PANU"),
    (0x1116, "NAP"),
    (0x1117, "GN"),
    (0x1118, "DirectPrinting"),
    (0x1119, "ReferencePrinting"),
    (0x111A, "Basic Imaging Profile"),
    (0x111B, "ImagingResponder"),
    (0x111C, "ImagingAutomaticArchive"),
    (0x111D, "ImagingReferencedObjects"),
    (0x111E, "Handsfree"),
    (0x111F, "HandsfreeAudioGateway"),
    (0x1120, "DirectPrintingReferenceObjectsService"),
    (0x1121, "ReflectedUI"),
    (0x1122, "BasicPrinting"),
    (0x1123, "PrintingStatus"),
    (0x1124, "HumanInterfaceDeviceService"),
    (0x1125, "HardcopyCableReplacement"),
    (0x1126, "HCR_Print"),
    (0x1127, "HCR_Scan"),
    (0x1128, "Common_ISDN_Access"),
    (0x112D, "SIM_Access"),
    (0x112E, "Phonebook Access - PCE"),
    (0x112F, "Phonebook Access - PSE"),
    (0x1130, "Phonebook Access"),
    (0x1131, "Headset - HS"),
    (0x1132, "Message Access Server"),
    (0x1133, "Message Notification Server"),
    (0x1134, "Message Access Profile"),
    (0x1135, "GNSS"),
    (0x1136, "GNSS_Server"),
    (0x1137, "3D Display"),
    (0x1138, "3D Glasses"),
    (0x1139, "3D Synchronization"),
    (0x113A, "MPS Profile"),
    (0x113B, "MPS SC"),
    (0x113C, "CTN Access Service"),
    (0x113D, "CTN Notification Service"),
    (0x113E, "CTN Profile"),
    (0x1200, "PnPInformation"),
    (0x1201, "GenericNetworking"),
    (0x1202, "GenericFileTransfer"),
    (0x1203, "GenericAudio"),
    (0x1204, "GenericTelephony"),
    (0x1205, "UPNP_Service"),
    (0x1206, "UPNP_IP_Service"),
    (0x1300, "ESDP_UPNP_IP_PAN"),
    (0x1301, "ESDP_UPNP_IP_LAP"),
    (0x1302, "ESDP_UPNP_L2CAP"),
    (0x1303, "VideoSource"),
    (0x1304, "VideoSink"),
    (0x1305, "VideoDistribution"),
    (0x1400, "HDP"),
    (0x1401, "HDP Source"),
    (0x1402, "HDP Sink"),
];

/// The assigned name for a 16-bit UUID, if well-known.
pub fn uuid16_name(uuid16: u16) -> Option<&'static str> {
    WELL_KNOWN_UUIDS
        .binary_search_by_key(&uuid16, |(u, _)| *u)
        .ok()
        .map(|i| WELL_KNOWN_UUIDS[i].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_and_sorted() {
        assert_eq!(uuid16_name(0x0001), Some("SDP"));
        assert_eq!(uuid16_name(0xFFFF), None);
        assert!(WELL_KNOWN_UUIDS.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
