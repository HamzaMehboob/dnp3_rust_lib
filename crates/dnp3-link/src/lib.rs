//! Link-layer frame encoding and decoding.

use dnp3_core::{
    decode_crc_blocks, encode_crc_blocks, verify_crc, Address, ProtocolError, Reader, Result,
};

/// DNP3 link-layer start bytes.
pub const START_BYTES: [u8; 2] = [0x05, 0x64];

const MIN_LENGTH_FIELD: usize = 5;
const MAX_LENGTH_FIELD: usize = 255;

/// Link-layer traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Master/client to outstation/server.
    MasterToOutstation,
    /// Outstation/server to master/client.
    OutstationToMaster,
}

impl Direction {
    fn bit(self) -> u8 {
        match self {
            Self::MasterToOutstation => 0,
            Self::OutstationToMaster => 0x80,
        }
    }
}

/// Primary link-layer function codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryFunction {
    /// Reset link-layer state.
    ResetLinkStates,
    /// Reset user process state.
    ResetUserProcess,
    /// Test link-layer state.
    TestLinkStates,
    /// User data requiring link-layer confirmation.
    ConfirmedUserData,
    /// User data that does not require link-layer confirmation.
    UnconfirmedUserData,
    /// Request link status.
    RequestLinkStatus,
}

impl PrimaryFunction {
    fn code(self) -> u8 {
        match self {
            Self::ResetLinkStates => 0,
            Self::ResetUserProcess => 1,
            Self::TestLinkStates => 2,
            Self::ConfirmedUserData => 3,
            Self::UnconfirmedUserData => 4,
            Self::RequestLinkStatus => 9,
        }
    }

    fn parse(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::ResetLinkStates),
            1 => Ok(Self::ResetUserProcess),
            2 => Ok(Self::TestLinkStates),
            3 => Ok(Self::ConfirmedUserData),
            4 => Ok(Self::UnconfirmedUserData),
            9 => Ok(Self::RequestLinkStatus),
            _ => Err(ProtocolError::UnknownValue {
                field: "primary link function",
                value,
            }),
        }
    }
}

/// Secondary link-layer function codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondaryFunction {
    /// Positive acknowledgement.
    Ack,
    /// Negative acknowledgement.
    Nack,
    /// Link status response.
    LinkStatus,
    /// Function is not supported.
    NotSupported,
}

impl SecondaryFunction {
    fn code(self) -> u8 {
        match self {
            Self::Ack => 0,
            Self::Nack => 1,
            Self::LinkStatus => 11,
            Self::NotSupported => 15,
        }
    }

    fn parse(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Ack),
            1 => Ok(Self::Nack),
            11 => Ok(Self::LinkStatus),
            15 => Ok(Self::NotSupported),
            _ => Err(ProtocolError::UnknownValue {
                field: "secondary link function",
                value,
            }),
        }
    }
}

/// Parsed link-layer function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFunction {
    /// Primary function.
    Primary(PrimaryFunction),
    /// Secondary function.
    Secondary(SecondaryFunction),
}

/// Link-layer control field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlField {
    /// Direction bit.
    pub direction: Direction,
    /// Frame count bit.
    pub fcb: bool,
    /// Frame count valid bit.
    pub fcv: bool,
    /// Function code.
    pub function: LinkFunction,
}

impl ControlField {
    /// Builds a primary control field.
    pub const fn primary(
        direction: Direction,
        function: PrimaryFunction,
        fcb: bool,
        fcv: bool,
    ) -> Self {
        Self {
            direction,
            fcb,
            fcv,
            function: LinkFunction::Primary(function),
        }
    }

    /// Builds a secondary control field.
    pub const fn secondary(direction: Direction, function: SecondaryFunction) -> Self {
        Self {
            direction,
            fcb: false,
            fcv: false,
            function: LinkFunction::Secondary(function),
        }
    }

    /// Encodes the control field into one byte.
    pub fn encode(self) -> u8 {
        let (prm, function) = match self.function {
            LinkFunction::Primary(function) => (0x40, function.code()),
            LinkFunction::Secondary(function) => (0x00, function.code()),
        };
        self.direction.bit()
            | prm
            | if self.fcb { 0x20 } else { 0 }
            | if self.fcv { 0x10 } else { 0 }
            | function
    }

    /// Decodes a control field from one byte.
    pub fn decode(value: u8) -> Result<Self> {
        let direction = if value & 0x80 == 0 {
            Direction::MasterToOutstation
        } else {
            Direction::OutstationToMaster
        };
        let fcb = value & 0x20 != 0;
        let fcv = value & 0x10 != 0;
        let function_code = value & 0x0f;
        let function = if value & 0x40 != 0 {
            LinkFunction::Primary(PrimaryFunction::parse(function_code)?)
        } else {
            LinkFunction::Secondary(SecondaryFunction::parse(function_code)?)
        };
        Ok(Self {
            direction,
            fcb,
            fcv,
            function,
        })
    }
}

/// A decoded link-layer frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkFrame {
    /// Link-layer control field.
    pub control: ControlField,
    /// Destination address.
    pub destination: Address,
    /// Source address.
    pub source: Address,
    /// Link payload after CRC validation.
    pub payload: Vec<u8>,
}

impl LinkFrame {
    /// Creates a new link frame.
    pub fn new(
        control: ControlField,
        destination: Address,
        source: Address,
        payload: Vec<u8>,
    ) -> Result<Self> {
        if payload.len() + MIN_LENGTH_FIELD > MAX_LENGTH_FIELD {
            return Err(ProtocolError::InvalidLength {
                expected: MAX_LENGTH_FIELD - MIN_LENGTH_FIELD,
                actual: payload.len(),
            });
        }
        Ok(Self {
            control,
            destination,
            source,
            payload,
        })
    }

    /// Creates an ACK frame (IEEE style, DIR=1, control 0x80).
    pub fn ack(destination: Address, source: Address) -> Self {
        Self::ack_with_fcb(destination, source, false)
    }

    /// Creates a legacy ACK frame used by many Windows masters (DIR=0, control 0x00).
    pub fn legacy_ack(destination: Address, source: Address) -> Self {
        Self::legacy_ack_with_fcb(destination, source, false)
    }

    /// Creates an ACK frame, optionally mirroring the request FCB bit.
    pub fn ack_with_fcb(destination: Address, source: Address, fcb: bool) -> Self {
        Self {
            control: ControlField {
                direction: Direction::OutstationToMaster,
                fcb,
                fcv: false,
                function: LinkFunction::Secondary(SecondaryFunction::Ack),
            },
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates a legacy ACK frame, optionally mirroring the request FCB bit.
    pub fn legacy_ack_with_fcb(destination: Address, source: Address, fcb: bool) -> Self {
        Self {
            control: ControlField {
                direction: Direction::MasterToOutstation,
                fcb,
                fcv: false,
                function: LinkFunction::Secondary(SecondaryFunction::Ack),
            },
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates a link-status response frame.
    pub fn link_status(destination: Address, source: Address) -> Self {
        Self {
            control: ControlField::secondary(
                Direction::OutstationToMaster,
                SecondaryFunction::LinkStatus,
            ),
            destination,
            source,
            payload: vec![0xFF],
        }
    }

    /// Creates a reset-link-states frame (IEEE style, control 0x40).
    pub fn reset_link_states(destination: Address, source: Address) -> Self {
        Self {
            control: ControlField::primary(
                Direction::MasterToOutstation,
                PrimaryFunction::ResetLinkStates,
                false,
                false,
            ),
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates a tool-style reset-link-states frame (control 0xC0).
    pub fn tool_reset_link_states(destination: Address, source: Address) -> Self {
        Self {
            control: ControlField::primary(
                Direction::OutstationToMaster,
                PrimaryFunction::ResetLinkStates,
                false,
                false,
            ),
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates a test-link-states frame (IEEE style, control 0x52).
    pub fn test_link_states(destination: Address, source: Address, fcb: bool) -> Self {
        Self {
            control: ControlField::primary(
                Direction::MasterToOutstation,
                PrimaryFunction::TestLinkStates,
                fcb,
                true,
            ),
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates a tool-style test-link-states frame (control 0xD2).
    pub fn tool_test_link_states(destination: Address, source: Address, fcb: bool) -> Self {
        Self {
            control: ControlField::primary(
                Direction::OutstationToMaster,
                PrimaryFunction::TestLinkStates,
                fcb,
                true,
            ),
            destination,
            source,
            payload: Vec::new(),
        }
    }

    /// Creates an unconfirmed user data frame (IEEE style, control 0x44).
    pub fn unconfirmed_user_data(
        destination: Address,
        source: Address,
        payload: Vec<u8>,
    ) -> Result<Self> {
        Self::new(
            ControlField::primary(
                Direction::MasterToOutstation,
                PrimaryFunction::UnconfirmedUserData,
                false,
                false,
            ),
            destination,
            source,
            payload,
        )
    }

    /// Creates a tool-style master user data frame (control 0xC4).
    pub fn tool_unconfirmed_user_data(
        destination: Address,
        source: Address,
        payload: Vec<u8>,
    ) -> Result<Self> {
        Self::new(
            ControlField::primary(
                Direction::OutstationToMaster,
                PrimaryFunction::UnconfirmedUserData,
                false,
                false,
            ),
            destination,
            source,
            payload,
        )
    }

    /// Creates an outstation response frame (outstation to master, control 0xC4).
    pub fn outstation_unconfirmed_user_data(
        destination: Address,
        source: Address,
        payload: Vec<u8>,
    ) -> Result<Self> {
        Self::new(
            ControlField::primary(
                Direction::OutstationToMaster,
                PrimaryFunction::UnconfirmedUserData,
                false,
                false,
            ),
            destination,
            source,
            payload,
        )
    }

    /// Encodes the frame to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let length = (MIN_LENGTH_FIELD + self.payload.len()) as u8;
        let mut header = Vec::with_capacity(8);
        header.extend_from_slice(&START_BYTES);
        header.push(length);
        header.push(self.control.encode());
        self.destination.encode(&mut header);
        self.source.encode(&mut header);

        let mut out = header.clone();
        dnp3_core::append_crc(&header, &mut out);
        encode_crc_blocks(&self.payload, &mut out);
        out
    }

    /// Decodes one complete frame from bytes.
    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.len() < 10 {
            return Err(ProtocolError::UnexpectedEof);
        }
        if input[..2] != START_BYTES {
            return Err(ProtocolError::InvalidStartBytes);
        }

        let length = usize::from(input[2]);
        if !(MIN_LENGTH_FIELD..=MAX_LENGTH_FIELD).contains(&length) {
            return Err(ProtocolError::InvalidLength {
                expected: MIN_LENGTH_FIELD,
                actual: length,
            });
        }

        verify_crc(&input[..8], &input[8..10])?;
        let payload_len = length - MIN_LENGTH_FIELD;
        let encoded_payload_len = encoded_payload_len(payload_len);
        let expected_len = 10 + encoded_payload_len;
        if input.len() != expected_len {
            return Err(ProtocolError::InvalidLength {
                expected: expected_len,
                actual: input.len(),
            });
        }

        let mut reader = Reader::new(&input[3..8]);
        let control = ControlField::decode(reader.read_u8()?)?;
        let destination = Address::decode(&mut reader)?;
        let source = Address::decode(&mut reader)?;
        let payload = decode_crc_blocks(&input[10..], payload_len)?;

        Ok(Self {
            control,
            destination,
            source,
            payload,
        })
    }
}

/// Returns the encoded byte count for a CRC-protected payload length.
pub fn encoded_payload_len(payload_len: usize) -> usize {
    if payload_len == 0 {
        0
    } else {
        payload_len + (payload_len.div_ceil(dnp3_core::CRC_BLOCK_SIZE) * 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn control_field_parsing_primary() {
        let control = ControlField::decode(0xf3).unwrap();
        assert_eq!(control.direction, Direction::OutstationToMaster);
        assert!(control.fcb);
        assert!(control.fcv);
        assert_eq!(
            control.function,
            LinkFunction::Primary(PrimaryFunction::ConfirmedUserData)
        );
        assert_eq!(control.encode(), 0xf3);
    }

    #[test]
    fn control_field_parsing_secondary() {
        let control = ControlField::decode(0x80).unwrap();
        assert_eq!(
            control.function,
            LinkFunction::Secondary(SecondaryFunction::Ack)
        );
        assert_eq!(control.encode(), 0x80);
    }

    #[test]
    fn link_frame_round_trip() {
        let frame = LinkFrame::unconfirmed_user_data(
            Address::new(1),
            Address::new(1024),
            (0_u8..40).collect(),
        )
        .unwrap();
        let encoded = frame.encode();
        assert_eq!(LinkFrame::decode(&encoded).unwrap(), frame);
    }

    #[test]
    fn invalid_crc_rejection() {
        let frame =
            LinkFrame::unconfirmed_user_data(Address::new(1), Address::new(2), vec![1]).unwrap();
        let mut encoded = frame.encode();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xff;
        assert_eq!(LinkFrame::decode(&encoded), Err(ProtocolError::InvalidCrc));
    }

    #[test]
    fn tool_reset_link_states_matches_freyr_client_capture() {
        let frame = LinkFrame::tool_reset_link_states(Address::new(10), Address::new(1));
        assert_eq!(frame.control.encode(), 0xC0);
        assert_eq!(
            frame.encode(),
            [0x05, 0x64, 0x05, 0xc0, 0x0a, 0x00, 0x01, 0x00, 0xb1, 0xac]
        );
    }

    #[test]
    fn invalid_length_rejection() {
        let frame =
            LinkFrame::unconfirmed_user_data(Address::new(1), Address::new(2), vec![1]).unwrap();
        let mut encoded = frame.encode();
        encoded[2] = 4;
        let crc = dnp3_core::crc16(&encoded[..8]).to_le_bytes();
        encoded[8..10].copy_from_slice(&crc);
        assert!(matches!(
            LinkFrame::decode(&encoded),
            Err(ProtocolError::InvalidLength { .. })
        ));
    }

    proptest! {
        #[test]
        fn round_trip_payload(payload in proptest::collection::vec(any::<u8>(), 0..=250)) {
            let frame = LinkFrame::unconfirmed_user_data(Address::new(7), Address::new(9), payload).unwrap();
            let encoded = frame.encode();
            prop_assert_eq!(LinkFrame::decode(&encoded).unwrap(), frame);
        }

        #[test]
        fn malformed_frames_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..=300)) {
            let _ = LinkFrame::decode(&bytes);
        }
    }
}
