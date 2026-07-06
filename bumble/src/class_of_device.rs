//! Class of Device (Vol 2, Part C - Assigned Numbers).
//!
//! Ported from `bumble.core.ClassOfDevice`. The 24-bit value packs:
//! `major_service_classes << 13 | major_device_class << 8 | minor << 2`.
//!
//! The string form matches Bumble exactly, e.g.
//! `"ClassOfDevice(RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER)"`. A minor value with
//! no known name renders in Python `hex()` style (`0x123`).

use core::fmt;

/// Major service classes bitfield (open). `composite_name` joins the set-bit
/// member names in definition (ascending-bit) order, matching Bumble.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MajorServiceClasses(pub u16);

impl MajorServiceClasses {
    pub const LIMITED_DISCOVERABLE_MODE: MajorServiceClasses = MajorServiceClasses(1 << 0);
    pub const LE_AUDIO: MajorServiceClasses = MajorServiceClasses(1 << 1);
    pub const POSITIONING: MajorServiceClasses = MajorServiceClasses(1 << 3);
    pub const NETWORKING: MajorServiceClasses = MajorServiceClasses(1 << 4);
    pub const RENDERING: MajorServiceClasses = MajorServiceClasses(1 << 5);
    pub const CAPTURING: MajorServiceClasses = MajorServiceClasses(1 << 6);
    pub const OBJECT_TRANSFER: MajorServiceClasses = MajorServiceClasses(1 << 7);
    pub const AUDIO: MajorServiceClasses = MajorServiceClasses(1 << 8);
    pub const TELEPHONY: MajorServiceClasses = MajorServiceClasses(1 << 9);
    pub const INFORMATION: MajorServiceClasses = MajorServiceClasses(1 << 10);

    /// Members in definition order, with their names, for `composite_name`.
    const MEMBERS: &'static [(u16, &'static str)] = &[
        (1 << 0, "LIMITED_DISCOVERABLE_MODE"),
        (1 << 1, "LE_AUDIO"),
        (1 << 3, "POSITIONING"),
        (1 << 4, "NETWORKING"),
        (1 << 5, "RENDERING"),
        (1 << 6, "CAPTURING"),
        (1 << 7, "OBJECT_TRANSFER"),
        (1 << 8, "AUDIO"),
        (1 << 9, "TELEPHONY"),
        (1 << 10, "INFORMATION"),
    ];

    /// `|`-joined names of the set bits, in ascending-bit order.
    pub fn composite_name(&self) -> String {
        Self::MEMBERS
            .iter()
            .filter(|(bit, _)| self.0 & bit != 0)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join("|")
    }
}

impl core::ops::BitOr for MajorServiceClasses {
    type Output = MajorServiceClasses;
    fn bitor(self, rhs: Self) -> Self {
        MajorServiceClasses(self.0 | rhs.0)
    }
}

/// Major device class (open enum, newtype over `u8`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MajorDeviceClass(pub u8);

impl MajorDeviceClass {
    pub const MISCELLANEOUS: MajorDeviceClass = MajorDeviceClass(0x00);
    pub const COMPUTER: MajorDeviceClass = MajorDeviceClass(0x01);
    pub const PHONE: MajorDeviceClass = MajorDeviceClass(0x02);
    pub const LAN_NETWORK_ACCESS_POINT: MajorDeviceClass = MajorDeviceClass(0x03);
    pub const AUDIO_VIDEO: MajorDeviceClass = MajorDeviceClass(0x04);
    pub const PERIPHERAL: MajorDeviceClass = MajorDeviceClass(0x05);
    pub const IMAGING: MajorDeviceClass = MajorDeviceClass(0x06);
    pub const WEARABLE: MajorDeviceClass = MajorDeviceClass(0x07);
    pub const TOY: MajorDeviceClass = MajorDeviceClass(0x08);
    pub const HEALTH: MajorDeviceClass = MajorDeviceClass(0x09);
    pub const UNCATEGORIZED: MajorDeviceClass = MajorDeviceClass(0x1F);

    fn name(&self) -> String {
        match self.0 {
            0x00 => "MISCELLANEOUS",
            0x01 => "COMPUTER",
            0x02 => "PHONE",
            0x03 => "LAN_NETWORK_ACCESS_POINT",
            0x04 => "AUDIO_VIDEO",
            0x05 => "PERIPHERAL",
            0x06 => "IMAGING",
            0x07 => "WEARABLE",
            0x08 => "TOY",
            0x09 => "HEALTH",
            0x1F => "UNCATEGORIZED",
            other => return format!("MajorDeviceClass[{other}]"),
        }
        .to_string()
    }
}

/// Named minor-device-class for a given major class, if known.
fn minor_device_class_name(major: MajorDeviceClass, minor: u32) -> Option<&'static str> {
    match major {
        MajorDeviceClass::AUDIO_VIDEO => Some(match minor {
            0x00 => "UNCATEGORIZED",
            0x01 => "WEARABLE_HEADSET_DEVICE",
            0x02 => "HANDS_FREE_DEVICE",
            0x04 => "MICROPHONE",
            0x05 => "LOUDSPEAKER",
            0x06 => "HEADPHONES",
            0x07 => "PORTABLE_AUDIO",
            0x08 => "CAR_AUDIO",
            0x09 => "SET_TOP_BOX",
            0x0A => "HIFI_AUDIO_DEVICE",
            0x0B => "VCR",
            0x0C => "VIDEO_CAMERA",
            0x0D => "CAMCORDER",
            0x0E => "VIDEO_MONITOR",
            0x0F => "VIDEO_DISPLAY_AND_LOUDSPEAKER",
            0x10 => "VIDEO_CONFERENCING",
            0x12 => "GAMING_OR_TOY",
            _ => return None,
        }),
        _ => None,
    }
}

/// A Class of Device value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassOfDevice {
    major_service_classes: MajorServiceClasses,
    major_device_class: MajorDeviceClass,
    minor_device_class: u32,
}

impl ClassOfDevice {
    /// Construct from the three components. `minor_device_class` is stored
    /// verbatim (not masked), matching Bumble's constructor.
    pub fn new(
        major_service_classes: MajorServiceClasses,
        major_device_class: MajorDeviceClass,
        minor_device_class: u32,
    ) -> ClassOfDevice {
        ClassOfDevice {
            major_service_classes,
            major_device_class,
            minor_device_class,
        }
    }

    /// Decode a 24-bit packed value.
    pub fn from_int(value: u32) -> ClassOfDevice {
        ClassOfDevice {
            major_service_classes: MajorServiceClasses(((value >> 13) & 0x7FF) as u16),
            major_device_class: MajorDeviceClass(((value >> 8) & 0x1F) as u8),
            minor_device_class: (value >> 2) & 0x3F,
        }
    }

    /// Encode to a packed value.
    pub fn to_int(&self) -> u32 {
        ((self.major_service_classes.0 as u32) << 13)
            | ((self.major_device_class.0 as u32) << 8)
            | (self.minor_device_class << 2)
    }

    pub fn major_service_classes(&self) -> MajorServiceClasses {
        self.major_service_classes
    }

    pub fn major_device_class(&self) -> MajorDeviceClass {
        self.major_device_class
    }

    pub fn minor_device_class(&self) -> u32 {
        self.minor_device_class
    }
}

impl fmt::Display for ClassOfDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minor_name = minor_device_class_name(self.major_device_class, self.minor_device_class)
            .map(str::to_string)
            // Python renders a raw-int minor via hex(): lowercase "0x…".
            .unwrap_or_else(|| format!("0x{:x}", self.minor_device_class));

        write!(
            f,
            "ClassOfDevice({},{}/{})",
            self.major_service_classes.composite_name(),
            self.major_device_class.name(),
            minor_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from bumble tests/core_test.py::test_class_of_device
    #[test]
    fn test_class_of_device() {
        let c1 = ClassOfDevice::new(
            MajorServiceClasses::AUDIO | MajorServiceClasses::RENDERING,
            MajorDeviceClass::AUDIO_VIDEO,
            0x0D, // CAMCORDER
        );
        assert_eq!(
            c1.to_string(),
            "ClassOfDevice(RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER)"
        );

        let c2 = ClassOfDevice::new(
            MajorServiceClasses::AUDIO,
            MajorDeviceClass::AUDIO_VIDEO,
            0x123,
        );
        assert_eq!(c2.to_string(), "ClassOfDevice(AUDIO,AUDIO_VIDEO/0x123)");
    }
}
