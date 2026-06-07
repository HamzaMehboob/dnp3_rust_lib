//! Core protocol primitives shared by the DNP3 workspace.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;

/// Number of payload bytes covered by one DNP3 data CRC.
pub const CRC_BLOCK_SIZE: usize = 16;

/// Common result type for protocol operations.
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Errors produced while parsing, encoding, or validating protocol data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The input ended before the requested field could be read.
    #[error("unexpected end of input")]
    UnexpectedEof,
    /// A length field or buffer size was outside the allowed range.
    #[error("invalid length: expected {expected}, actual {actual}")]
    InvalidLength { expected: usize, actual: usize },
    /// A CRC did not match the bytes it protected.
    #[error("invalid crc")]
    InvalidCrc,
    /// A start byte sequence was not recognized.
    #[error("invalid start bytes")]
    InvalidStartBytes,
    /// A numeric enum discriminant is not supported.
    #[error("unknown value {value:#04x} for {field}")]
    UnknownValue { field: &'static str, value: u8 },
    /// A sequence number did not match the expected value.
    #[error("invalid sequence: expected {expected}, actual {actual}")]
    InvalidSequence { expected: u8, actual: u8 },
    /// A fragment arrived in an invalid order.
    #[error("invalid fragment sequence")]
    InvalidFragment,
    /// A timeout elapsed.
    #[error("operation timed out")]
    Timeout,
    /// A transport endpoint was closed.
    #[error("transport closed")]
    Closed,
}

/// A DNP3 endpoint address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address(u16);

impl Address {
    /// Broadcast to all stations.
    pub const BROADCAST: Self = Self(0xffff);

    /// Creates a new address.
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the raw 16-bit address.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Encodes the address in little-endian order.
    pub fn encode(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0.to_le_bytes());
    }

    /// Decodes an address from little-endian bytes.
    pub fn decode(input: &mut Reader<'_>) -> Result<Self> {
        Ok(Self(input.read_u16_le()?))
    }
}

impl From<u16> for Address {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A millisecond timestamp encoded as 48-bit little-endian time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp48(u64);

impl Timestamp48 {
    /// Largest value that fits in the 48-bit wire format.
    pub const MAX_VALUE: u64 = 0x0000_ffff_ffff_ffff;

    /// Creates a timestamp from milliseconds since the Unix epoch.
    pub fn from_millis(value: u64) -> Result<Self> {
        if value <= Self::MAX_VALUE {
            Ok(Self(value))
        } else {
            Err(ProtocolError::InvalidLength {
                expected: Self::MAX_VALUE as usize,
                actual: value as usize,
            })
        }
    }

    /// Returns the timestamp value in milliseconds.
    pub const fn millis(self) -> u64 {
        self.0
    }

    /// Returns the current system time, truncated to the 48-bit wire range.
    pub fn now() -> Self {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        Self((elapsed.as_millis() as u64) & Self::MAX_VALUE)
    }

    /// Encodes the timestamp in 6 little-endian bytes.
    pub fn encode(self, out: &mut Vec<u8>) {
        let bytes = self.0.to_le_bytes();
        out.extend_from_slice(&bytes[..6]);
    }

    /// Decodes the timestamp from 6 little-endian bytes.
    pub fn decode(input: &mut Reader<'_>) -> Result<Self> {
        let bytes = input.read_exact(6)?;
        let mut expanded = [0_u8; 8];
        expanded[..6].copy_from_slice(bytes);
        Ok(Self(u64::from_le_bytes(expanded)))
    }
}

/// A small deterministic byte reader for protocol parsers.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    /// Creates a new reader over a byte slice.
    pub const fn new(input: &'a [u8]) -> Self {
        Self { input, cursor: 0 }
    }

    /// Returns the number of unread bytes.
    pub fn remaining(&self) -> usize {
        self.input.len().saturating_sub(self.cursor)
    }

    /// Returns true if there are no unread bytes.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Skips runs of four zero bytes used as padding in some vendor responses.
    pub fn skip_padding_zeros(&mut self) {
        while self.remaining() >= 4 {
            let start = self.cursor;
            if self.input[start..start + 4] == [0, 0, 0, 0] {
                self.cursor += 4;
            } else {
                break;
            }
        }
    }

    /// Reads exactly `len` bytes.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.remaining() < len {
            return Err(ProtocolError::UnexpectedEof);
        }
        let start = self.cursor;
        self.cursor += len;
        Ok(&self.input[start..self.cursor])
    }

    /// Reads one byte.
    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    /// Reads a little-endian `u16`.
    pub fn read_u16_le(&mut self) -> Result<u16> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a little-endian `u32`.
    pub fn read_u32_le(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a little-endian `i16`.
    pub fn read_i16_le(&mut self) -> Result<i16> {
        let bytes = self.read_exact(2)?;
        Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a little-endian `i32`.
    pub fn read_i32_le(&mut self) -> Result<i32> {
        let bytes = self.read_exact(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

/// Computes the DNP3 CRC for one protected block.
pub fn crc16(block: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for byte in block {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xa6bc;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Appends the little-endian CRC for a protected block.
pub fn append_crc(block: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&crc16(block).to_le_bytes());
}

/// Verifies a little-endian CRC for a protected block.
pub fn verify_crc(block: &[u8], crc: &[u8]) -> Result<()> {
    if crc.len() != 2 {
        return Err(ProtocolError::InvalidLength {
            expected: 2,
            actual: crc.len(),
        });
    }
    let actual = u16::from_le_bytes([crc[0], crc[1]]);
    let expected = crc16(block);
    if actual == expected {
        Ok(())
    } else {
        Err(ProtocolError::InvalidCrc)
    }
}

/// Encodes data with one CRC after every 16 payload bytes.
pub fn encode_crc_blocks(data: &[u8], out: &mut Vec<u8>) {
    for chunk in data.chunks(CRC_BLOCK_SIZE) {
        out.extend_from_slice(chunk);
        append_crc(chunk, out);
    }
}

/// Decodes data protected by one CRC after every 16 payload bytes.
pub fn decode_crc_blocks(encoded: &[u8], payload_len: usize) -> Result<Vec<u8>> {
    let mut decoded = Vec::with_capacity(payload_len);
    let mut offset = 0;
    while decoded.len() < payload_len {
        let block_len = (payload_len - decoded.len()).min(CRC_BLOCK_SIZE);
        if encoded.len().saturating_sub(offset) < block_len + 2 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let block = &encoded[offset..offset + block_len];
        let crc = &encoded[offset + block_len..offset + block_len + 2];
        verify_crc(block, crc)?;
        decoded.extend_from_slice(block);
        offset += block_len + 2;
    }
    if offset != encoded.len() {
        return Err(ProtocolError::InvalidLength {
            expected: offset,
            actual: encoded.len(),
        });
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_vector_header() {
        let header = [0x05, 0x64, 0x05, 0xc0, 0x01, 0x00, 0x00, 0x04];
        assert_eq!(crc16(&header), 0x21e9);
    }

    #[test]
    fn crc_blocks_round_trip() {
        let data: Vec<u8> = (0..40).collect();
        let mut encoded = Vec::new();
        encode_crc_blocks(&data, &mut encoded);
        assert_eq!(decode_crc_blocks(&encoded, data.len()).unwrap(), data);
    }

    #[test]
    fn invalid_crc_is_rejected() {
        let data = [1, 2, 3, 4];
        assert_eq!(verify_crc(&data, &[0, 0]), Err(ProtocolError::InvalidCrc));
    }

    #[test]
    fn address_round_trip() {
        let mut encoded = Vec::new();
        Address::new(1024).encode(&mut encoded);
        let mut reader = Reader::new(&encoded);
        assert_eq!(Address::decode(&mut reader).unwrap(), Address::new(1024));
    }

    #[test]
    fn timestamp_round_trip() {
        let time = Timestamp48::from_millis(0x0102_0304_0506).unwrap();
        let mut encoded = Vec::new();
        time.encode(&mut encoded);
        let mut reader = Reader::new(&encoded);
        assert_eq!(Timestamp48::decode(&mut reader).unwrap(), time);
    }
}
