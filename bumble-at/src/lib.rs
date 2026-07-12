//! AT command parameter and HFP command/response parsing.
//!
//! The parameter grammar is a direct port of `google/bumble/bumble/at.py`:
//! spaces outside quotes are ignored, quoted strings preserve separators, and
//! parentheses produce nested parameter lists. The command/response models and
//! streaming delimiters are the protocol-neutral AT pieces previously housed
//! in upstream `hfp.py`.

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    InvalidPacket(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPacket(message) => write!(f, "invalid AT packet: {message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// One AT parameter: raw bytes or a parenthesized nested parameter list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parameter {
    Value(Vec<u8>),
    List(Vec<Parameter>),
}

impl From<&[u8]> for Parameter {
    fn from(value: &[u8]) -> Self {
        Parameter::Value(value.to_vec())
    }
}

impl<const N: usize> From<&[u8; N]> for Parameter {
    fn from(value: &[u8; N]) -> Self {
        Parameter::Value(value.to_vec())
    }
}

/// Split a parameter byte string into values and separator tokens, matching
/// upstream `at.tokenize_parameters` byte-for-byte.
pub fn tokenize_parameters(buffer: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut tokens = Vec::new();
    let mut in_quotes = false;
    let mut token = Vec::new();

    for &byte in buffer {
        if in_quotes {
            token.push(byte);
            if byte == b'"' {
                in_quotes = false;
                tokens.push(token[1..token.len() - 1].to_vec());
                token.clear();
            }
            continue;
        }

        match byte {
            b' ' => {}
            b',' | b')' => {
                tokens.push(core::mem::take(&mut token));
                tokens.push(vec![byte]);
            }
            b'(' => {
                if !token.is_empty() {
                    return Err(Error::InvalidPacket(
                        "open_paren following regular character".into(),
                    ));
                }
                tokens.push(vec![byte]);
            }
            b'"' => {
                if !token.is_empty() {
                    return Err(Error::InvalidPacket(
                        "quote following regular character".into(),
                    ));
                }
                in_quotes = true;
                token.push(byte);
            }
            _ => token.push(byte),
        }
    }

    // Upstream does not reject a missing closing quote; it returns the
    // accumulated token including the opening quote.
    tokens.push(token);
    tokens.retain(|token| !token.is_empty());
    Ok(tokens)
}

/// Parse comma-separated and parenthesized AT parameters.
pub fn parse_parameters(buffer: &[u8]) -> Result<Vec<Parameter>> {
    let tokens = tokenize_parameters(buffer)?;
    let mut accumulator: Vec<Vec<Parameter>> = vec![Vec::new()];
    let mut current = Parameter::Value(Vec::new());

    for token in tokens {
        match token.as_slice() {
            b"," => {
                accumulator
                    .last_mut()
                    .expect("root accumulator exists")
                    .push(current);
                current = Parameter::Value(Vec::new());
            }
            b"(" => accumulator.push(Vec::new()),
            b")" => {
                if accumulator.len() < 2 {
                    return Err(Error::InvalidPacket(
                        "close_paren without matching open_paren".into(),
                    ));
                }
                accumulator
                    .last_mut()
                    .expect("nested accumulator exists")
                    .push(current);
                current = Parameter::List(accumulator.pop().expect("nested list exists"));
            }
            _ => current = Parameter::Value(token),
        }
    }

    accumulator
        .last_mut()
        .expect("root accumulator exists")
        .push(current);
    if accumulator.len() > 1 {
        return Err(Error::InvalidPacket("missing close_paren".into()));
    }
    Ok(accumulator.pop().expect("root list exists"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSubCode {
    None,
    Set,
    Test,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtCommand {
    pub code: String,
    pub sub_code: CommandSubCode,
    pub parameters: Vec<Parameter>,
}

impl AtCommand {
    /// Parse the HFP AT command forms from upstream `AtCommand.parse_from`.
    pub fn parse(buffer: &[u8]) -> Result<Self> {
        if buffer.starts_with(b"ATA") {
            return Ok(Self {
                code: "A".into(),
                sub_code: CommandSubCode::None,
                parameters: Vec::new(),
            });
        }
        if buffer.starts_with(b"ATD") {
            return Ok(Self {
                code: "D".into(),
                sub_code: CommandSubCode::None,
                parameters: vec![Parameter::Value(buffer[3..].to_vec())],
            });
        }
        let suffix = buffer
            .strip_prefix(b"AT+")
            .ok_or_else(|| Error::InvalidPacket("invalid command".into()))?;
        let code_end = suffix
            .iter()
            .position(|byte| !byte.is_ascii_uppercase())
            .unwrap_or(suffix.len());
        if code_end == 0 {
            return Err(Error::InvalidPacket("invalid command".into()));
        }
        let code = core::str::from_utf8(&suffix[..code_end])
            .map_err(|_| Error::InvalidPacket("command code is not ASCII".into()))?
            .to_owned();
        let remainder = &suffix[code_end..];
        let (sub_code, parameters) = if let Some(parameters) = remainder.strip_prefix(b"=?") {
            (CommandSubCode::Test, parameters)
        } else if let Some(parameters) = remainder.strip_prefix(b"=") {
            (CommandSubCode::Set, parameters)
        } else if let Some(parameters) = remainder.strip_prefix(b"?") {
            (CommandSubCode::Read, parameters)
        } else {
            (CommandSubCode::None, remainder)
        };
        let parameters = if parameters.is_empty() {
            Vec::new()
        } else {
            parse_parameters(parameters)?
        };
        Ok(Self {
            code,
            sub_code,
            parameters,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtResponse {
    pub code: String,
    pub parameters: Vec<Parameter>,
}

impl AtResponse {
    pub fn parse(buffer: &[u8]) -> Result<Self> {
        let mut fields = buffer.split(|byte| *byte == b':');
        let code = fields.next().unwrap_or_default();
        let parameters = fields.next().unwrap_or_default();
        Ok(Self {
            code: core::str::from_utf8(code)
                .map_err(|_| Error::InvalidPacket("response code is not UTF-8".into()))?
                .to_owned(),
            parameters: parse_parameters(parameters)?,
        })
    }
}

/// Incremental parser for commands terminated by carriage return (`\r`).
#[derive(Debug, Default)]
pub struct CommandStream {
    buffer: Vec<u8>,
}

impl CommandStream {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<AtCommand>> {
        self.buffer.extend_from_slice(bytes);
        let mut commands = Vec::new();
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\r') {
            let raw = self.buffer[..end].to_vec();
            self.buffer.drain(..=end);
            commands.push(AtCommand::parse(&raw)?);
        }
        Ok(commands)
    }
}

/// Incremental parser for responses framed as `\r\n...\r\n`.
#[derive(Debug, Default)]
pub struct ResponseStream {
    buffer: Vec<u8>,
}

impl ResponseStream {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<AtResponse>> {
        self.buffer.extend_from_slice(bytes);
        let mut responses = Vec::new();
        loop {
            let Some(header) = find_crlf(&self.buffer, 0) else {
                break;
            };
            let Some(trailer) = find_crlf(&self.buffer, header + 2) else {
                break;
            };
            let raw = self.buffer[header + 2..trailer].to_vec();
            self.buffer.drain(..trailer + 2);
            responses.push(AtResponse::parse(&raw)?);
        }
        Ok(responses)
    }
}

fn find_crlf(buffer: &[u8], start: usize) -> Option<usize> {
    buffer
        .get(start..)?
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}
