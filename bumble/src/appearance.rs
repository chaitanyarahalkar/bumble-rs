//! GAP Appearance (assigned numbers).
//!
//! Ported from `bumble.core.Appearance`. The 16-bit appearance value packs a
//! 10-bit category and a 6-bit subcategory: `value = (category << 6) | sub`.
//! Note the encoding is asymmetric — the constructor / [`Appearance::to_int`]
//! do **not** mask the subcategory, matching Bumble.
//!
//! `Category` and the subcategory space are *open*: unknown values are
//! preserved. When a subcategory has no known name, its string form is
//! `"<SubcategoryClassName>[<decimal>]"`, mirroring Bumble's `OpenIntEnum`.

use core::fmt;

/// Appearance category. Open enum (newtype over `u16`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Category(pub u16);

impl Category {
    pub const UNKNOWN: Category = Category(0x0000);
    pub const PHONE: Category = Category(0x0001);
    pub const COMPUTER: Category = Category(0x0002);
    pub const WATCH: Category = Category(0x0003);
    pub const CLOCK: Category = Category(0x0004);
    pub const DISPLAY: Category = Category(0x0005);
    pub const REMOTE_CONTROL: Category = Category(0x0006);
    pub const EYE_GLASSES: Category = Category(0x0007);
    pub const TAG: Category = Category(0x0008);
    pub const KEYRING: Category = Category(0x0009);
    pub const MEDIA_PLAYER: Category = Category(0x000A);
    pub const BARCODE_SCANNER: Category = Category(0x000B);
    pub const THERMOMETER: Category = Category(0x000C);
    pub const HEART_RATE_SENSOR: Category = Category(0x000D);
    pub const BLOOD_PRESSURE: Category = Category(0x000E);
    pub const HUMAN_INTERFACE_DEVICE: Category = Category(0x000F);
    pub const GLUCOSE_METER: Category = Category(0x0010);
    pub const RUNNING_WALKING_SENSOR: Category = Category(0x0011);
    pub const CYCLING: Category = Category(0x0012);
}

/// Human-readable name for a category value (its Bumble enum member name).
fn category_name(v: u16) -> Option<&'static str> {
    Some(match v {
        0x0000 => "UNKNOWN",
        0x0001 => "PHONE",
        0x0002 => "COMPUTER",
        0x0003 => "WATCH",
        0x0004 => "CLOCK",
        0x0005 => "DISPLAY",
        0x0006 => "REMOTE_CONTROL",
        0x0007 => "EYE_GLASSES",
        0x0008 => "TAG",
        0x0009 => "KEYRING",
        0x000A => "MEDIA_PLAYER",
        0x000B => "BARCODE_SCANNER",
        0x000C => "THERMOMETER",
        0x000D => "HEART_RATE_SENSOR",
        0x000E => "BLOOD_PRESSURE",
        0x000F => "HUMAN_INTERFACE_DEVICE",
        0x0010 => "GLUCOSE_METER",
        0x0011 => "RUNNING_WALKING_SENSOR",
        0x0012 => "CYCLING",
        _ => return None,
    })
}

/// Known subcategory member name for a `(category, subcategory)` pair.
fn subcategory_name(category: u16, sub: u16) -> Option<&'static str> {
    match category {
        // ComputerSubcategory
        0x0002 => Some(match sub {
            0x00 => "GENERIC_COMPUTER",
            0x01 => "DESKTOP_WORKSTATION",
            0x02 => "SERVER_CLASS_COMPUTER",
            0x03 => "LAPTOP",
            0x04 => "HANDHELD_PC_PDA",
            0x05 => "PALM_SIZE_PC_PDA",
            0x06 => "WEARABLE_COMPUTER",
            0x07 => "TABLET",
            0x08 => "DOCKING_STATION",
            0x09 => "ALL_IN_ONE",
            0x0A => "BLADE_SERVER",
            0x0B => "CONVERTIBLE",
            0x0C => "DETACHABLE",
            0x0D => "IOT_GATEWAY",
            0x0E => "MINI_PC",
            0x0F => "STICK_PC",
            _ => return None,
        }),
        // BloodPressureSubcategory
        0x000E => Some(match sub {
            0x00 => "GENERIC_BLOOD_PRESSURE",
            0x01 => "ARM_BLOOD_PRESSURE",
            0x02 => "WRIST_BLOOD_PRESSURE",
            _ => return None,
        }),
        _ => None,
    }
}

/// The Bumble subcategory-class name for a category, used for the
/// `"<Class>[<n>]"` fallback when a subcategory value has no known name.
fn subcategory_class_name(category: u16) -> Option<&'static str> {
    Some(match category {
        0x0002 => "ComputerSubcategory",
        0x000E => "BloodPressureSubcategory",
        0x000F => "HumanInterfaceDeviceSubcategory",
        _ => return None,
    })
}

/// A GAP Appearance: a category plus a subcategory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Appearance {
    category: Category,
    subcategory: u16,
}

impl Appearance {
    /// Construct from a category and subcategory. The subcategory is stored
    /// verbatim (not masked to 6 bits), matching Bumble.
    pub fn new(category: Category, subcategory: u16) -> Appearance {
        Appearance {
            category,
            subcategory,
        }
    }

    /// Decode a packed 16-bit appearance value: `category = v >> 6`,
    /// `subcategory = v & 0x3F`.
    pub fn from_int(value: u16) -> Appearance {
        Appearance {
            category: Category(value >> 6),
            subcategory: value & 0x3F,
        }
    }

    /// Decode from 2 little-endian bytes.
    pub fn from_bytes(data: &[u8]) -> Appearance {
        let v = u16::from_le_bytes([
            data.first().copied().unwrap_or(0),
            data.get(1).copied().unwrap_or(0),
        ]);
        Appearance::from_int(v)
    }

    /// The category.
    pub fn category(&self) -> Category {
        self.category
    }

    /// The subcategory value.
    pub fn subcategory(&self) -> u16 {
        self.subcategory
    }

    /// Encode to a packed 16-bit value: `(category << 6) | subcategory`
    /// (no masking, matching Bumble).
    pub fn to_int(&self) -> u16 {
        (self.category.0 << 6) | self.subcategory
    }

    /// Encode to 2 little-endian bytes.
    pub fn to_bytes(&self) -> [u8; 2] {
        self.to_int().to_le_bytes()
    }
}

impl fmt::Display for Appearance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cat = category_name(self.category.0)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Category[{}]", self.category.0));

        let sub = if let Some(name) = subcategory_name(self.category.0, self.subcategory) {
            name.to_string()
        } else if let Some(class) = subcategory_class_name(self.category.0) {
            format!("{}[{}]", class, self.subcategory)
        } else {
            format!("{}", self.subcategory)
        };

        write!(f, "{cat}/{sub}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from bumble tests/core_test.py::test_appearance
    #[test]
    fn test_appearance() {
        let a = Appearance::new(Category::COMPUTER, 0x03 /* LAPTOP */);
        assert_eq!(a.to_string(), "COMPUTER/LAPTOP");
        assert_eq!(a.to_int(), 0x0083);

        let a = Appearance::new(Category::HUMAN_INTERFACE_DEVICE, 0x77);
        assert_eq!(
            a.to_string(),
            "HUMAN_INTERFACE_DEVICE/HumanInterfaceDeviceSubcategory[119]"
        );
        assert_eq!(a.to_int(), 0x03C0 | 0x77);

        let a = Appearance::from_int(0x0381);
        assert_eq!(a.category(), Category::BLOOD_PRESSURE);
        assert_eq!(a.subcategory(), 0x01 /* ARM_BLOOD_PRESSURE */);
        assert_eq!(a.to_int(), 0x381);

        let a = Appearance::from_int(0x038A);
        assert_eq!(a.category(), Category::BLOOD_PRESSURE);
        assert_eq!(a.subcategory(), 0x0A);
        assert_eq!(a.to_int(), 0x038A);

        let a = Appearance::from_int(0x3333);
        assert_eq!(a.category(), Category(0xCC));
        assert_eq!(a.subcategory(), 0x33);
        assert_eq!(a.to_int(), 0x3333);
    }
}
