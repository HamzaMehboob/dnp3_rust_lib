//! Transport-layer segmentation and reassembly.

use dnp3_core::{ProtocolError, Result};

/// Maximum transport payload carried by one link payload.
pub const MAX_TRANSPORT_PAYLOAD: usize = 249;

/// Transport header flags and sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportHeader {
    /// First segment in an application fragment.
    pub fir: bool,
    /// Final segment in an application fragment.
    pub fin: bool,
    /// Six-bit sequence number.
    pub sequence: u8,
}

impl TransportHeader {
    /// Creates a new transport header.
    pub fn new(fir: bool, fin: bool, sequence: u8) -> Self {
        Self {
            fir,
            fin,
            sequence: sequence & 0x3f,
        }
    }

    /// Encodes the header into one byte.
    pub fn encode(self) -> u8 {
        (if self.fir { 0x80 } else { 0 })
            | (if self.fin { 0x40 } else { 0 })
            | (self.sequence & 0x3f)
    }

    /// Decodes one byte into a transport header.
    pub fn decode(value: u8) -> Self {
        Self {
            fir: value & 0x80 != 0,
            fin: value & 0x40 != 0,
            sequence: value & 0x3f,
        }
    }
}

/// A transport segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportSegment {
    /// Segment header.
    pub header: TransportHeader,
    /// Segment payload.
    pub payload: Vec<u8>,
}

impl TransportSegment {
    /// Encodes a segment into bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.payload.len());
        out.push(self.header.encode());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Decodes bytes into one segment.
    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.is_empty() {
            return Err(ProtocolError::UnexpectedEof);
        }
        if input.len() > MAX_TRANSPORT_PAYLOAD + 1 {
            return Err(ProtocolError::InvalidLength {
                expected: MAX_TRANSPORT_PAYLOAD + 1,
                actual: input.len(),
            });
        }
        Ok(Self {
            header: TransportHeader::decode(input[0]),
            payload: input[1..].to_vec(),
        })
    }
}

/// Splits application bytes into transport segments.
pub fn fragment(application: &[u8], start_sequence: u8) -> Vec<TransportSegment> {
    if application.is_empty() {
        return vec![TransportSegment {
            header: TransportHeader::new(true, true, start_sequence),
            payload: Vec::new(),
        }];
    }

    let chunks: Vec<&[u8]> = application.chunks(MAX_TRANSPORT_PAYLOAD).collect();
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| TransportSegment {
            header: TransportHeader::new(
                index == 0,
                index + 1 == chunks.len(),
                start_sequence.wrapping_add(index as u8),
            ),
            payload: chunk.to_vec(),
        })
        .collect()
}

/// Reassembles transport segments into application fragments.
#[derive(Debug, Default)]
pub struct Reassembler {
    expected_sequence: Option<u8>,
    buffer: Vec<u8>,
}

impl Reassembler {
    /// Creates an empty reassembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all partial state.
    pub fn reset(&mut self) {
        self.expected_sequence = None;
        self.buffer.clear();
    }

    /// Pushes one segment. Returns `Ok(Some(fragment))` when reassembly completes.
    pub fn push(&mut self, segment: TransportSegment) -> Result<Option<Vec<u8>>> {
        if segment.header.fir {
            self.buffer.clear();
            self.expected_sequence = Some(segment.header.sequence);
        } else if self.expected_sequence.is_none() {
            return Err(ProtocolError::InvalidFragment);
        }

        let expected = self
            .expected_sequence
            .ok_or(ProtocolError::InvalidFragment)?;
        if segment.header.sequence != expected {
            self.reset();
            return Err(ProtocolError::InvalidSequence {
                expected,
                actual: segment.header.sequence,
            });
        }

        self.buffer.extend_from_slice(&segment.payload);
        self.expected_sequence = Some((expected + 1) & 0x3f);

        if segment.header.fin {
            let mut complete = Vec::new();
            std::mem::swap(&mut complete, &mut self.buffer);
            self.expected_sequence = None;
            Ok(Some(complete))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn sequence_header_round_trip() {
        let header = TransportHeader::new(true, false, 63);
        assert_eq!(TransportHeader::decode(header.encode()), header);
    }

    #[test]
    fn fragment_reassembly() {
        let payload: Vec<u8> = (0..=255).cycle().take(700).collect();
        let segments = fragment(&payload, 12);
        assert!(segments.len() > 1);
        let mut reassembler = Reassembler::new();
        let mut completed = None;
        for segment in segments {
            completed = reassembler.push(segment).unwrap();
        }
        assert_eq!(completed.unwrap(), payload);
    }

    #[test]
    fn invalid_sequence_rejected() {
        let mut reassembler = Reassembler::new();
        let first = TransportSegment {
            header: TransportHeader::new(true, false, 1),
            payload: vec![1],
        };
        let second = TransportSegment {
            header: TransportHeader::new(false, true, 3),
            payload: vec![2],
        };
        assert_eq!(reassembler.push(first).unwrap(), None);
        assert!(matches!(
            reassembler.push(second),
            Err(ProtocolError::InvalidSequence { .. })
        ));
    }

    proptest! {
        #[test]
        fn round_trip_fragmentation(payload in proptest::collection::vec(any::<u8>(), 0..=2048), seq in 0_u8..64) {
            let segments = fragment(&payload, seq);
            let mut reassembler = Reassembler::new();
            let mut out = None;
            for segment in segments {
                out = reassembler.push(segment).unwrap();
            }
            prop_assert_eq!(out.unwrap(), payload);
        }
    }
}
