//! Typed characteristic and proxy adapters from upstream `gatt_adapters.py`.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use crate::{
    AttTransport, CharacteristicDefinition, CharacteristicProxy, DynamicValue, GattClient,
    GattError,
};

const ATT_UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Debug, PartialEq)]
pub enum AdapterError {
    MissingEncoder,
    MissingDecoder,
    InvalidFormat(String),
    InvalidValue(String),
    InvalidUtf8(String),
    Codec(String),
    Gatt(GattError),
    StatePoisoned,
}

impl core::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingEncoder => f.write_str("adapter does not have an encoder"),
            Self::MissingDecoder => f.write_str("adapter does not have a decoder"),
            Self::InvalidFormat(message) => write!(f, "invalid packed format: {message}"),
            Self::InvalidValue(message) => write!(f, "invalid adapted value: {message}"),
            Self::InvalidUtf8(message) => write!(f, "invalid UTF-8: {message}"),
            Self::Codec(message) => write!(f, "codec error: {message}"),
            Self::Gatt(error) => error.fmt(f),
            Self::StatePoisoned => f.write_str("adapter state lock is poisoned"),
        }
    }
}

impl std::error::Error for AdapterError {}

impl From<GattError> for AdapterError {
    fn from(value: GattError) -> Self {
        Self::Gatt(value)
    }
}

pub trait ValueCodec: Clone + Send + Sync + 'static {
    type Value: Send + 'static;

    fn encode(&self, value: &Self::Value) -> Result<Vec<u8>, AdapterError>;
    fn decode(&self, value: &[u8]) -> Result<Self::Value, AdapterError>;
}

/// A typed view over a discovered raw characteristic proxy.
#[derive(Clone, Debug)]
pub struct CharacteristicProxyAdapter<C: ValueCodec> {
    proxy: CharacteristicProxy,
    codec: C,
}

impl<C: ValueCodec> CharacteristicProxyAdapter<C> {
    pub fn new(proxy: CharacteristicProxy, codec: C) -> Self {
        Self { proxy, codec }
    }

    pub fn proxy(&self) -> &CharacteristicProxy {
        &self.proxy
    }

    pub fn read_value(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        no_long_read: bool,
    ) -> Result<C::Value, AdapterError> {
        let bytes = client.read_value(transport, self.proxy.handle, no_long_read)?;
        self.codec.decode(&bytes)
    }

    pub fn write_value(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        value: &C::Value,
        with_response: bool,
    ) -> Result<(), AdapterError> {
        client.write_value(
            transport,
            self.proxy.handle,
            self.codec.encode(value)?,
            with_response,
        )?;
        Ok(())
    }

    pub fn decode_cached(&self, client: &GattClient) -> Result<Option<C::Value>, AdapterError> {
        client
            .cached_value(self.proxy.handle)
            .map(|value| self.codec.decode(value))
            .transpose()
    }
}

/// A typed server characteristic definition plus its codec.
#[derive(Clone, Debug)]
pub struct CharacteristicAdapter<C: ValueCodec> {
    pub definition: CharacteristicDefinition,
    codec: C,
}

impl<C: ValueCodec> CharacteristicAdapter<C> {
    pub fn new(definition: CharacteristicDefinition, codec: C) -> Self {
        Self { definition, codec }
    }

    pub fn encode_value(&self, value: &C::Value) -> Result<Vec<u8>, AdapterError> {
        self.codec.encode(value)
    }

    pub fn decode_value(&self, value: &[u8]) -> Result<C::Value, AdapterError> {
        self.codec.decode(value)
    }

    /// Make callbacks that keep a typed value in shared state. Bind the result
    /// to this characteristic's assigned handle with `set_dynamic_value`.
    pub fn dynamic_value(&self, state: Arc<Mutex<C::Value>>) -> DynamicValue {
        let read_state = Arc::clone(&state);
        let write_state = state;
        let read_codec = self.codec.clone();
        let write_codec = self.codec.clone();
        DynamicValue::read_write(
            move |_| {
                let value = read_state.lock().map_err(|_| ATT_UNLIKELY_ERROR)?;
                read_codec.encode(&value).map_err(|_| ATT_UNLIKELY_ERROR)
            },
            move |_, bytes| {
                let value = write_codec.decode(bytes).map_err(|_| ATT_UNLIKELY_ERROR)?;
                *write_state.lock().map_err(|_| ATT_UNLIKELY_ERROR)? = value;
                Ok(())
            },
        )
    }
}

type Encoder<T> = dyn Fn(&T) -> Result<Vec<u8>, AdapterError> + Send + Sync;
type Decoder<T> = dyn Fn(&[u8]) -> Result<T, AdapterError> + Send + Sync;

pub struct DelegatedCodec<T> {
    encode: Option<Arc<Encoder<T>>>,
    decode: Option<Arc<Decoder<T>>>,
}

impl<T> Clone for DelegatedCodec<T> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
        }
    }
}

impl<T> core::fmt::Debug for DelegatedCodec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DelegatedCodec")
            .field("encode", &self.encode.is_some())
            .field("decode", &self.decode.is_some())
            .finish()
    }
}

impl<T> DelegatedCodec<T> {
    pub fn new<E, D>(encode: E, decode: D) -> Self
    where
        E: Fn(&T) -> Result<Vec<u8>, AdapterError> + Send + Sync + 'static,
        D: Fn(&[u8]) -> Result<T, AdapterError> + Send + Sync + 'static,
    {
        Self {
            encode: Some(Arc::new(encode)),
            decode: Some(Arc::new(decode)),
        }
    }

    pub fn encoder<E>(encode: E) -> Self
    where
        E: Fn(&T) -> Result<Vec<u8>, AdapterError> + Send + Sync + 'static,
    {
        Self {
            encode: Some(Arc::new(encode)),
            decode: None,
        }
    }

    pub fn decoder<D>(decode: D) -> Self
    where
        D: Fn(&[u8]) -> Result<T, AdapterError> + Send + Sync + 'static,
    {
        Self {
            encode: None,
            decode: Some(Arc::new(decode)),
        }
    }
}

impl<T: Send + 'static> ValueCodec for DelegatedCodec<T> {
    type Value = T;

    fn encode(&self, value: &T) -> Result<Vec<u8>, AdapterError> {
        self.encode.as_ref().ok_or(AdapterError::MissingEncoder)?(value)
    }

    fn decode(&self, value: &[u8]) -> Result<T, AdapterError> {
        self.decode.as_ref().ok_or(AdapterError::MissingDecoder)?(value)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Utf8Codec;

impl ValueCodec for Utf8Codec {
    type Value = String;

    fn encode(&self, value: &String) -> Result<Vec<u8>, AdapterError> {
        Ok(value.as_bytes().to_vec())
    }

    fn decode(&self, value: &[u8]) -> Result<String, AdapterError> {
        String::from_utf8(value.to_vec())
            .map_err(|error| AdapterError::InvalidUtf8(error.to_string()))
    }
}

pub trait ByteSerializable: Sized + Send + 'static {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, String>;
}

#[derive(Debug)]
pub struct SerializableCodec<T>(PhantomData<fn() -> T>);

impl<T> Clone for SerializableCodec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SerializableCodec<T> {}

impl<T> Default for SerializableCodec<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: ByteSerializable> ValueCodec for SerializableCodec<T> {
    type Value = T;

    fn encode(&self, value: &T) -> Result<Vec<u8>, AdapterError> {
        Ok(value.to_bytes())
    }

    fn decode(&self, value: &[u8]) -> Result<T, AdapterError> {
        T::from_bytes(value).map_err(AdapterError::Codec)
    }
}

pub trait IntConvertible: Sized + Send + 'static {
    fn to_u64(&self) -> u64;
    fn from_u64(value: u64) -> Result<Self, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    Little,
    Big,
}

#[derive(Debug)]
pub struct EnumCodec<T> {
    length: usize,
    byte_order: ByteOrder,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for EnumCodec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for EnumCodec<T> {}

impl<T> EnumCodec<T> {
    pub fn new(length: usize, byte_order: ByteOrder) -> Result<Self, AdapterError> {
        if !(1..=8).contains(&length) {
            return Err(AdapterError::InvalidValue(
                "enum byte length must be between 1 and 8".into(),
            ));
        }
        Ok(Self {
            length,
            byte_order,
            marker: PhantomData,
        })
    }
}

impl<T: IntConvertible> ValueCodec for EnumCodec<T> {
    type Value = T;

    fn encode(&self, value: &T) -> Result<Vec<u8>, AdapterError> {
        let integer = value.to_u64();
        if self.length < 8 && integer >= (1u64 << (self.length * 8)) {
            return Err(AdapterError::InvalidValue(format!(
                "enum value {integer} does not fit in {} bytes",
                self.length
            )));
        }
        let bytes = match self.byte_order {
            ByteOrder::Little => integer.to_le_bytes(),
            ByteOrder::Big => integer.to_be_bytes(),
        };
        Ok(match self.byte_order {
            ByteOrder::Little => bytes[..self.length].to_vec(),
            ByteOrder::Big => bytes[8 - self.length..].to_vec(),
        })
    }

    fn decode(&self, value: &[u8]) -> Result<T, AdapterError> {
        if value.len() != self.length {
            return Err(AdapterError::InvalidValue(format!(
                "expected {} enum bytes, got {}",
                self.length,
                value.len()
            )));
        }
        let mut bytes = [0u8; 8];
        match self.byte_order {
            ByteOrder::Little => bytes[..self.length].copy_from_slice(value),
            ByteOrder::Big => bytes[8 - self.length..].copy_from_slice(value),
        }
        let integer = match self.byte_order {
            ByteOrder::Little => u64::from_le_bytes(bytes),
            ByteOrder::Big => u64::from_be_bytes(bytes),
        };
        T::from_u64(integer).map_err(AdapterError::Codec)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PackedValue {
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Complex(f64, f64),
    Bytes(Vec<u8>),
    Tuple(Vec<PackedValue>),
}

#[derive(Clone, Copy, Debug)]
enum FieldKind {
    Pad,
    Bool,
    Signed,
    Unsigned,
    Float16,
    Float32,
    Float64,
    Complex32,
    Complex64,
    Bytes,
    Pascal,
    Char,
}

#[derive(Clone, Copy, Debug)]
struct Field {
    kind: FieldKind,
    size: usize,
}

#[derive(Clone, Debug)]
pub struct PackedCodec {
    byte_order: ByteOrder,
    fields: Vec<Field>,
    value_count: usize,
    size: usize,
}

fn native_byte_order() -> ByteOrder {
    if cfg!(target_endian = "little") {
        ByteOrder::Little
    } else {
        ByteOrder::Big
    }
}

fn append_alignment(
    fields: &mut Vec<Field>,
    offset: &mut usize,
    alignment: usize,
) -> Result<(), AdapterError> {
    let padding = (alignment - (*offset % alignment)) % alignment;
    if padding != 0 {
        fields.push(Field {
            kind: FieldKind::Pad,
            size: padding,
        });
        *offset = offset
            .checked_add(padding)
            .ok_or_else(|| AdapterError::InvalidFormat("packed size overflow".into()))?;
    }
    Ok(())
}

impl PackedCodec {
    pub fn new(format: &str) -> Result<Self, AdapterError> {
        let mut chars = format.chars().peekable();
        let (byte_order, native) = match chars.peek().copied() {
            Some('<') => {
                chars.next();
                (ByteOrder::Little, false)
            }
            Some('>') | Some('!') => {
                chars.next();
                (ByteOrder::Big, false)
            }
            Some('=') => {
                chars.next();
                (native_byte_order(), false)
            }
            Some('@') => {
                chars.next();
                (native_byte_order(), true)
            }
            _ => (native_byte_order(), true),
        };

        let mut fields = Vec::new();
        let mut offset = 0usize;
        let native_long_size = if cfg!(target_os = "windows") {
            4
        } else {
            core::mem::size_of::<isize>()
        };
        let pointer_size = core::mem::size_of::<usize>();
        while chars.peek().is_some() {
            if chars.peek().is_some_and(|ch| ch.is_whitespace()) {
                chars.next();
                continue;
            }
            let mut count = 0usize;
            let mut has_count = false;
            while let Some(ch) = chars.peek().copied().filter(char::is_ascii_digit) {
                has_count = true;
                chars.next();
                count = count
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(ch.to_digit(10).unwrap() as usize))
                    .ok_or_else(|| AdapterError::InvalidFormat("repeat count overflow".into()))?;
            }
            let count = if has_count { count } else { 1 };
            let code = chars
                .next()
                .ok_or_else(|| AdapterError::InvalidFormat("missing format code".into()))?;
            let (kind, size, repeated, alignment) = match code {
                'x' => (FieldKind::Pad, 1, true, 1),
                '?' => (FieldKind::Bool, 1, true, 1),
                'b' => (FieldKind::Signed, 1, true, 1),
                'B' => (FieldKind::Unsigned, 1, true, 1),
                'h' => (FieldKind::Signed, 2, true, 2),
                'H' => (FieldKind::Unsigned, 2, true, 2),
                'i' => (FieldKind::Signed, 4, true, 4),
                'I' => (FieldKind::Unsigned, 4, true, 4),
                'l' => (
                    FieldKind::Signed,
                    if native { native_long_size } else { 4 },
                    true,
                    if native { native_long_size } else { 1 },
                ),
                'L' => (
                    FieldKind::Unsigned,
                    if native { native_long_size } else { 4 },
                    true,
                    if native { native_long_size } else { 1 },
                ),
                'q' => (FieldKind::Signed, 8, true, 8),
                'Q' => (FieldKind::Unsigned, 8, true, 8),
                'n' if native => (FieldKind::Signed, pointer_size, true, pointer_size),
                'N' | 'P' if native => (FieldKind::Unsigned, pointer_size, true, pointer_size),
                'n' | 'N' | 'P' => {
                    return Err(AdapterError::InvalidFormat(format!(
                        "format code '{code}' requires native '@' mode"
                    )))
                }
                'e' => (FieldKind::Float16, 2, true, 2),
                'f' => (FieldKind::Float32, 4, true, 4),
                'd' => (FieldKind::Float64, 8, true, 8),
                'F' => (FieldKind::Complex32, 8, true, 4),
                'D' => (FieldKind::Complex64, 16, true, 8),
                'c' => (FieldKind::Char, 1, true, 1),
                's' => (FieldKind::Bytes, count, false, 1),
                'p' => (FieldKind::Pascal, count, false, 1),
                other => {
                    return Err(AdapterError::InvalidFormat(format!(
                        "unsupported format code '{other}'"
                    )))
                }
            };
            if repeated {
                if count == 0 && native && !matches!(kind, FieldKind::Pad) {
                    append_alignment(&mut fields, &mut offset, alignment)?;
                }
                for _ in 0..count {
                    if native {
                        append_alignment(&mut fields, &mut offset, alignment)?;
                    }
                    fields.push(Field { kind, size });
                    offset = offset.checked_add(size).ok_or_else(|| {
                        AdapterError::InvalidFormat("packed size overflow".into())
                    })?;
                }
            } else {
                fields.push(Field { kind, size });
                offset = offset
                    .checked_add(size)
                    .ok_or_else(|| AdapterError::InvalidFormat("packed size overflow".into()))?;
            }
        }
        let size = offset;
        let value_count = fields
            .iter()
            .filter(|field| !matches!(field.kind, FieldKind::Pad))
            .count();
        Ok(Self {
            byte_order,
            fields,
            value_count,
            size,
        })
    }

    pub fn size(&self) -> usize {
        self.size
    }

    fn values<'a>(&self, value: &'a PackedValue) -> Result<Vec<&'a PackedValue>, AdapterError> {
        match (self.value_count, value) {
            (1, PackedValue::Tuple(_)) => Err(AdapterError::InvalidValue(
                "single-field format requires a scalar".into(),
            )),
            (1, scalar) => Ok(vec![scalar]),
            (_, PackedValue::Tuple(values)) if values.len() == self.value_count => {
                Ok(values.iter().collect())
            }
            (_, PackedValue::Tuple(values)) => Err(AdapterError::InvalidValue(format!(
                "expected {} values, got {}",
                self.value_count,
                values.len()
            ))),
            _ => Err(AdapterError::InvalidValue(format!(
                "format requires {} values",
                self.value_count
            ))),
        }
    }
}

impl ValueCodec for PackedCodec {
    type Value = PackedValue;

    fn encode(&self, value: &PackedValue) -> Result<Vec<u8>, AdapterError> {
        let values = self.values(value)?;
        let mut value_index = 0usize;
        let mut output = Vec::with_capacity(self.size);
        for field in &self.fields {
            if matches!(field.kind, FieldKind::Pad) {
                output.resize(output.len() + field.size, 0);
                continue;
            }
            let value = values[value_index];
            value_index += 1;
            encode_field(&mut output, *field, value, self.byte_order)?;
        }
        Ok(output)
    }

    fn decode(&self, value: &[u8]) -> Result<PackedValue, AdapterError> {
        if value.len() != self.size {
            return Err(AdapterError::InvalidValue(format!(
                "expected {} packed bytes, got {}",
                self.size,
                value.len()
            )));
        }
        let mut offset = 0usize;
        let mut values = Vec::with_capacity(self.value_count);
        for field in &self.fields {
            let bytes = &value[offset..offset + field.size];
            offset += field.size;
            if !matches!(field.kind, FieldKind::Pad) {
                values.push(decode_field(*field, bytes, self.byte_order));
            }
        }
        Ok(if values.len() == 1 {
            values.remove(0)
        } else {
            PackedValue::Tuple(values)
        })
    }
}

fn encode_field(
    output: &mut Vec<u8>,
    field: Field,
    value: &PackedValue,
    order: ByteOrder,
) -> Result<(), AdapterError> {
    match (field.kind, value) {
        (FieldKind::Bool, PackedValue::Bool(value)) => output.push(u8::from(*value)),
        (FieldKind::Signed, PackedValue::Signed(value)) => {
            let minimum = if field.size == 8 {
                i64::MIN
            } else {
                -(1i64 << (field.size * 8 - 1))
            };
            let maximum = if field.size == 8 {
                i64::MAX
            } else {
                (1i64 << (field.size * 8 - 1)) - 1
            };
            if !(minimum..=maximum).contains(value) {
                return Err(AdapterError::InvalidValue("signed integer overflow".into()));
            }
            append_integer(output, *value as u64, field.size, order);
        }
        (FieldKind::Unsigned, PackedValue::Unsigned(value)) => {
            if field.size < 8 && *value >= (1u64 << (field.size * 8)) {
                return Err(AdapterError::InvalidValue(
                    "unsigned integer overflow".into(),
                ));
            }
            append_integer(output, *value, field.size, order);
        }
        (FieldKind::Float16, PackedValue::Float(value)) => {
            append_integer(output, u64::from(f32_to_f16(*value as f32)?), 2, order)
        }
        (FieldKind::Float32, PackedValue::Float(value)) => {
            append_integer(output, (*value as f32).to_bits() as u64, 4, order)
        }
        (FieldKind::Float64, PackedValue::Float(value)) => {
            append_integer(output, value.to_bits(), 8, order)
        }
        (FieldKind::Complex32, PackedValue::Complex(real, imaginary)) => {
            append_integer(output, (*real as f32).to_bits() as u64, 4, order);
            append_integer(output, (*imaginary as f32).to_bits() as u64, 4, order);
        }
        (FieldKind::Complex64, PackedValue::Complex(real, imaginary)) => {
            append_integer(output, real.to_bits(), 8, order);
            append_integer(output, imaginary.to_bits(), 8, order);
        }
        (FieldKind::Char, PackedValue::Bytes(value)) if value.len() == 1 => output.push(value[0]),
        (FieldKind::Bytes, PackedValue::Bytes(value)) => {
            output.extend_from_slice(&value[..value.len().min(field.size)]);
            output.resize(output.len() + field.size.saturating_sub(value.len()), 0);
        }
        (FieldKind::Pascal, PackedValue::Bytes(value)) => {
            if field.size == 0 {
                return Ok(());
            }
            let length = value.len().min(field.size - 1).min(255);
            output.push(length as u8);
            output.extend_from_slice(&value[..length]);
            output.resize(output.len() + field.size - 1 - length, 0);
        }
        _ => {
            return Err(AdapterError::InvalidValue(format!(
                "value {value:?} does not match field {field:?}"
            )))
        }
    }
    Ok(())
}

fn decode_field(field: Field, bytes: &[u8], order: ByteOrder) -> PackedValue {
    match field.kind {
        FieldKind::Bool => PackedValue::Bool(bytes[0] != 0),
        FieldKind::Signed => {
            let unsigned = read_integer(bytes, order);
            let shift = 64 - field.size * 8;
            PackedValue::Signed(((unsigned << shift) as i64) >> shift)
        }
        FieldKind::Unsigned => PackedValue::Unsigned(read_integer(bytes, order)),
        FieldKind::Float16 => {
            PackedValue::Float(f16_to_f32(read_integer(bytes, order) as u16) as f64)
        }
        FieldKind::Float32 => {
            PackedValue::Float(f32::from_bits(read_integer(bytes, order) as u32) as f64)
        }
        FieldKind::Float64 => PackedValue::Float(f64::from_bits(read_integer(bytes, order))),
        FieldKind::Complex32 => PackedValue::Complex(
            f32::from_bits(read_integer(&bytes[..4], order) as u32) as f64,
            f32::from_bits(read_integer(&bytes[4..], order) as u32) as f64,
        ),
        FieldKind::Complex64 => PackedValue::Complex(
            f64::from_bits(read_integer(&bytes[..8], order)),
            f64::from_bits(read_integer(&bytes[8..], order)),
        ),
        FieldKind::Char | FieldKind::Bytes => PackedValue::Bytes(bytes.to_vec()),
        FieldKind::Pascal => {
            if bytes.is_empty() {
                return PackedValue::Bytes(Vec::new());
            }
            let length = usize::from(bytes[0]).min(bytes.len().saturating_sub(1));
            PackedValue::Bytes(bytes[1..1 + length].to_vec())
        }
        FieldKind::Pad => unreachable!("padding does not produce a value"),
    }
}

fn append_integer(output: &mut Vec<u8>, value: u64, size: usize, order: ByteOrder) {
    let bytes = match order {
        ByteOrder::Little => value.to_le_bytes(),
        ByteOrder::Big => value.to_be_bytes(),
    };
    match order {
        ByteOrder::Little => output.extend_from_slice(&bytes[..size]),
        ByteOrder::Big => output.extend_from_slice(&bytes[8 - size..]),
    }
}

fn read_integer(value: &[u8], order: ByteOrder) -> u64 {
    let mut bytes = [0u8; 8];
    match order {
        ByteOrder::Little => bytes[..value.len()].copy_from_slice(value),
        ByteOrder::Big => bytes[8 - value.len()..].copy_from_slice(value),
    }
    match order {
        ByteOrder::Little => u64::from_le_bytes(bytes),
        ByteOrder::Big => u64::from_be_bytes(bytes),
    }
}

fn f32_to_f16(value: f32) -> Result<u16, AdapterError> {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x7F_FFFF;
    if exponent == 0xFF {
        if mantissa == 0 {
            return Ok(sign | 0x7C00);
        }
        let payload = ((mantissa >> 13) as u16) | 0x0200;
        return Ok(sign | 0x7C00 | payload);
    }

    let mut half_exponent = exponent - 127 + 15;
    if half_exponent >= 31 {
        return Err(AdapterError::InvalidValue(
            "float is outside the binary16 range".into(),
        ));
    }
    if half_exponent <= 0 {
        if half_exponent < -10 {
            return Ok(sign);
        }
        let significand = mantissa | 0x80_0000;
        let shift = (14 - half_exponent) as u32;
        let mut rounded = significand >> shift;
        let remainder = significand & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        if remainder > halfway || (remainder == halfway && rounded & 1 != 0) {
            rounded += 1;
        }
        return Ok(sign | rounded as u16);
    }

    let mut rounded = mantissa >> 13;
    let remainder = mantissa & 0x1FFF;
    if remainder > 0x1000 || (remainder == 0x1000 && rounded & 1 != 0) {
        rounded += 1;
        if rounded == 0x400 {
            rounded = 0;
            half_exponent += 1;
            if half_exponent >= 31 {
                return Err(AdapterError::InvalidValue(
                    "float is outside the binary16 range".into(),
                ));
            }
        }
    }
    Ok(sign | ((half_exponent as u16) << 10) | rounded as u16)
}

fn f16_to_f32(value: u16) -> f32 {
    let sign = (u32::from(value & 0x8000)) << 16;
    let exponent = u32::from((value >> 10) & 0x1F);
    let mantissa = u32::from(value & 0x03FF);
    let bits = match exponent {
        0 if mantissa == 0 => sign,
        0 => {
            let mut mantissa = mantissa;
            let mut exponent = -14i32;
            while mantissa & 0x0400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }
            mantissa &= 0x03FF;
            sign | (((exponent + 127) as u32) << 23) | (mantissa << 13)
        }
        0x1F => sign | 0x7F80_0000 | (mantissa << 13),
        _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
    };
    f32::from_bits(bits)
}

#[derive(Clone, Debug)]
pub struct MappedCodec {
    packed: PackedCodec,
    keys: Vec<String>,
}

impl MappedCodec {
    pub fn new(
        format: &str,
        keys: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, AdapterError> {
        let packed = PackedCodec::new(format)?;
        let keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        if keys.len() != packed.value_count {
            return Err(AdapterError::InvalidValue(format!(
                "expected {} mapping keys, got {}",
                packed.value_count,
                keys.len()
            )));
        }
        Ok(Self { packed, keys })
    }
}

impl ValueCodec for MappedCodec {
    type Value = BTreeMap<String, PackedValue>;

    fn encode(&self, value: &Self::Value) -> Result<Vec<u8>, AdapterError> {
        let values = self
            .keys
            .iter()
            .map(|key| {
                value
                    .get(key)
                    .cloned()
                    .ok_or_else(|| AdapterError::InvalidValue(format!("missing key '{key}'")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let packed = if values.len() == 1 {
            values.into_iter().next().unwrap()
        } else {
            PackedValue::Tuple(values)
        };
        self.packed.encode(&packed)
    }

    fn decode(&self, value: &[u8]) -> Result<Self::Value, AdapterError> {
        let unpacked = self.packed.decode(value)?;
        let values = match unpacked {
            PackedValue::Tuple(values) => values,
            scalar => vec![scalar],
        };
        Ok(self.keys.iter().cloned().zip(values).collect())
    }
}

pub type DelegatedCharacteristicProxyAdapter<T> = CharacteristicProxyAdapter<DelegatedCodec<T>>;
pub type PackedCharacteristicProxyAdapter = CharacteristicProxyAdapter<PackedCodec>;
pub type MappedCharacteristicProxyAdapter = CharacteristicProxyAdapter<MappedCodec>;
pub type Utf8CharacteristicProxyAdapter = CharacteristicProxyAdapter<Utf8Codec>;
pub type UTF8CharacteristicProxyAdapter = CharacteristicProxyAdapter<Utf8Codec>;
pub type SerializableCharacteristicProxyAdapter<T> =
    CharacteristicProxyAdapter<SerializableCodec<T>>;
pub type EnumCharacteristicProxyAdapter<T> = CharacteristicProxyAdapter<EnumCodec<T>>;

pub type DelegatedCharacteristicAdapter<T> = CharacteristicAdapter<DelegatedCodec<T>>;
pub type PackedCharacteristicAdapter = CharacteristicAdapter<PackedCodec>;
pub type MappedCharacteristicAdapter = CharacteristicAdapter<MappedCodec>;
pub type Utf8CharacteristicAdapter = CharacteristicAdapter<Utf8Codec>;
pub type UTF8CharacteristicAdapter = CharacteristicAdapter<Utf8Codec>;
pub type SerializableCharacteristicAdapter<T> = CharacteristicAdapter<SerializableCodec<T>>;
pub type EnumCharacteristicAdapter<T> = CharacteristicAdapter<EnumCodec<T>>;
