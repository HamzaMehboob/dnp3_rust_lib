//! Application-layer requests, responses, function codes, and object values.

use dnp3_core::{ProtocolError, Reader, Result, Timestamp48};

/// Application-layer control field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppControl {
    /// First fragment flag.
    pub fir: bool,
    /// Final fragment flag.
    pub fin: bool,
    /// Confirmation requested flag.
    pub con: bool,
    /// Unsolicited flag.
    pub uns: bool,
    /// Four-bit sequence number.
    pub sequence: u8,
}

impl AppControl {
    /// Creates a request/response control field.
    pub fn new(fir: bool, fin: bool, con: bool, uns: bool, sequence: u8) -> Self {
        Self {
            fir,
            fin,
            con,
            uns,
            sequence: sequence & 0x0f,
        }
    }

    /// Creates a single-fragment control field.
    pub fn single(sequence: u8) -> Self {
        Self::new(true, true, false, false, sequence)
    }

    /// Encodes the control field.
    pub fn encode(self) -> u8 {
        (if self.fir { 0x80 } else { 0 })
            | (if self.fin { 0x40 } else { 0 })
            | (if self.con { 0x20 } else { 0 })
            | (if self.uns { 0x10 } else { 0 })
            | (self.sequence & 0x0f)
    }

    /// Decodes a control field.
    pub fn decode(value: u8) -> Self {
        Self {
            fir: value & 0x80 != 0,
            fin: value & 0x40 != 0,
            con: value & 0x20 != 0,
            uns: value & 0x10 != 0,
            sequence: value & 0x0f,
        }
    }
}

/// Common application function codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionCode {
    /// Confirm an application fragment.
    Confirm,
    /// Read objects.
    Read,
    /// Write objects.
    Write,
    /// Select controllable outputs.
    Select,
    /// Operate selected outputs.
    Operate,
    /// Direct operate.
    DirectOperate,
    /// Freeze counters.
    Freeze,
    /// Enable unsolicited responses.
    EnableUnsolicited,
    /// Disable unsolicited responses.
    DisableUnsolicited,
    /// Assign class data.
    AssignClass,
    /// Delay measurement.
    DelayMeasure,
    /// Record current time.
    RecordCurrentTime,
    /// Response.
    Response,
    /// Unsolicited response.
    UnsolicitedResponse,
}

impl FunctionCode {
    /// Encodes the function code.
    pub fn encode(self) -> u8 {
        match self {
            Self::Confirm => 0,
            Self::Read => 1,
            Self::Write => 2,
            Self::Select => 3,
            Self::Operate => 4,
            Self::DirectOperate => 5,
            Self::Freeze => 7,
            Self::EnableUnsolicited => 20,
            Self::DisableUnsolicited => 21,
            Self::AssignClass => 22,
            Self::DelayMeasure => 23,
            Self::RecordCurrentTime => 24,
            Self::Response => 129,
            Self::UnsolicitedResponse => 130,
        }
    }

    /// Decodes a function code.
    pub fn decode(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Confirm),
            1 => Ok(Self::Read),
            2 => Ok(Self::Write),
            3 => Ok(Self::Select),
            4 => Ok(Self::Operate),
            5 => Ok(Self::DirectOperate),
            7 => Ok(Self::Freeze),
            20 => Ok(Self::EnableUnsolicited),
            21 => Ok(Self::DisableUnsolicited),
            22 => Ok(Self::AssignClass),
            23 => Ok(Self::DelayMeasure),
            24 => Ok(Self::RecordCurrentTime),
            129 => Ok(Self::Response),
            130 => Ok(Self::UnsolicitedResponse),
            _ => Err(ProtocolError::UnknownValue {
                field: "application function",
                value,
            }),
        }
    }
}

/// Object header qualifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qualifier {
    /// Start and stop index are encoded as one byte each.
    Range8 { start: u8, stop: u8 },
    /// Start and stop index are encoded as two bytes each.
    Range16 { start: u16, stop: u16 },
    /// No range or count fields.
    AllObjects,
    /// One-byte object count.
    Count8(u8),
    /// Two-byte object count.
    Count16(u16),
    /// One-byte count followed by two-byte index prefixes in the object body.
    CountAndPrefix16(u8),
    /// One 32-bit index prefix in the header (qualifier 0x14).
    Indexed32(u32),
    /// One-byte count followed by four-byte index prefixes in the object body.
    CountAndPrefix32(u8),
}

impl Qualifier {
    fn count(self) -> Result<usize> {
        match self {
            Self::Count8(count)
            | Self::CountAndPrefix16(count)
            | Self::CountAndPrefix32(count) => Ok(usize::from(count)),
            Self::Count16(count) => Ok(usize::from(count)),
            _ => Err(ProtocolError::UnknownValue {
                field: "object qualifier without count",
                value: self.code(),
            }),
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Range8 { .. } => 0x00,
            Self::Range16 { .. } => 0x01,
            Self::AllObjects => 0x06,
            Self::Count8(_) => 0x07,
            Self::Count16(_) => 0x08,
            Self::CountAndPrefix16(_) => 0x28,
            Self::CountAndPrefix32(_) => 0x38,
            Self::Indexed32(_) => 0x14,
        }
    }
}

/// Application object header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectHeader {
    /// Object group.
    pub group: u8,
    /// Object variation.
    pub variation: u8,
    /// Header qualifier.
    pub qualifier: Qualifier,
}

impl ObjectHeader {
    /// Creates a header.
    pub const fn new(group: u8, variation: u8, qualifier: Qualifier) -> Self {
        Self {
            group,
            variation,
            qualifier,
        }
    }

    /// Encodes the object header.
    pub fn encode(self, out: &mut Vec<u8>) {
        out.push(self.group);
        out.push(self.variation);
        out.push(self.qualifier.code());
        match self.qualifier {
            Qualifier::Range8 { start, stop } => {
                out.push(start);
                out.push(stop);
            }
            Qualifier::Range16 { start, stop } => {
                out.extend_from_slice(&start.to_le_bytes());
                out.extend_from_slice(&stop.to_le_bytes());
            }
            Qualifier::AllObjects => {}
            Qualifier::Count8(count) | Qualifier::CountAndPrefix16(count) => out.push(count),
            Qualifier::Count16(count) => out.extend_from_slice(&count.to_le_bytes()),
            Qualifier::CountAndPrefix32(count) => out.push(count),
            Qualifier::Indexed32(index) => out.extend_from_slice(&index.to_le_bytes()),
        }
    }

    /// Decodes an object header.
    pub fn decode(input: &mut Reader<'_>) -> Result<Self> {
        let group = input.read_u8()?;
        let variation = input.read_u8()?;
        let qualifier_code = input.read_u8()?;
        let qualifier = match qualifier_code {
            0x00 => Qualifier::Range8 {
                start: input.read_u8()?,
                stop: input.read_u8()?,
            },
            0x01 => Qualifier::Range16 {
                start: input.read_u16_le()?,
                stop: input.read_u16_le()?,
            },
            0x06 => Qualifier::AllObjects,
            0x07 => Qualifier::Count8(input.read_u8()?),
            0x08 => Qualifier::Count16(input.read_u16_le()?),
            0x14 => Qualifier::Indexed32(input.read_u32_le()?),
            code if is_simulator_indexed_qualifier(code) => {
                Qualifier::CountAndPrefix32(input.read_u8()?)
            }
            code if is_count_and_prefix16_qualifier(code) => {
                Qualifier::CountAndPrefix16(input.read_u8()?)
            }
            _ => {
                return Err(ProtocolError::UnknownValue {
                    field: "object qualifier",
                    value: qualifier_code,
                })
            }
        };
        Ok(Self {
            group,
            variation,
            qualifier,
        })
    }
}

/// Binary input point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryInput {
    /// Point index.
    pub index: u16,
    /// Flags byte.
    pub flags: u8,
}

/// Binary output point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryOutput {
    /// Point index.
    pub index: u16,
    /// Flags byte.
    pub flags: u8,
}

/// Analog input point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalogInput {
    /// Point index.
    pub index: u16,
    /// Flags byte.
    pub flags: u8,
    /// Signed 32-bit value.
    pub value: i32,
}

/// Analog output point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalogOutput {
    /// Point index.
    pub index: u16,
    /// Flags byte.
    pub flags: u8,
    /// Signed 32-bit value.
    pub value: i32,
}

/// Counter point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    /// Point index.
    pub index: u16,
    /// Flags byte.
    pub flags: u8,
    /// Unsigned 32-bit value.
    pub value: u32,
}

/// DNP3 event and static data classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClass {
    /// Static data.
    Class0,
    /// Event class 1.
    Class1,
    /// Event class 2.
    Class2,
    /// Event class 3.
    Class3,
}

impl DataClass {
    fn variation(self) -> u8 {
        match self {
            Self::Class0 => 1,
            Self::Class1 => 2,
            Self::Class2 => 3,
            Self::Class3 => 4,
        }
    }

    fn parse(variation: u8) -> Result<Self> {
        match variation {
            1 => Ok(Self::Class0),
            2 => Ok(Self::Class1),
            3 => Ok(Self::Class2),
            4 => Ok(Self::Class3),
            _ => Err(ProtocolError::UnknownValue {
                field: "class variation",
                value: variation,
            }),
        }
    }
}

/// Supported application-layer object bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppObject {
    /// Binary input values.
    BinaryInputs(Vec<BinaryInput>),
    /// Binary output values.
    BinaryOutputs(Vec<BinaryOutput>),
    /// Analog input values.
    AnalogInputs(Vec<AnalogInput>),
    /// Analog output values.
    AnalogOutputs(Vec<AnalogOutput>),
    /// Counter values.
    Counters(Vec<Counter>),
    /// Double-bit binary input values (group 3).
    DoubleBitBinaryInputs(Vec<BinaryInput>),
    /// Absolute time.
    Time(Timestamp48),
    /// Class scan request.
    ClassScan(DataClass),
    /// Integrity poll request.
    IntegrityPoll,
}

impl AppObject {
    /// Encodes one object body with its header.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::BinaryInputs(values) => encode_binary_like(1, 2, values, out),
            Self::DoubleBitBinaryInputs(values) => encode_binary_like(3, 2, values, out),
            Self::BinaryOutputs(values) => encode_binary_like(10, 2, values, out),
            Self::AnalogInputs(values) => encode_analog_like(30, 1, values, out),
            Self::AnalogOutputs(values) => encode_analog_like(40, 1, values, out),
            Self::Counters(values) => encode_counter_like(values, out),
            Self::Time(timestamp) => {
                ObjectHeader::new(50, 1, Qualifier::Count8(1)).encode(out);
                timestamp.encode(out);
                Ok(())
            }
            Self::ClassScan(class) => {
                ObjectHeader::new(60, class.variation(), Qualifier::AllObjects).encode(out);
                Ok(())
            }
            Self::IntegrityPoll => {
                ObjectHeader::new(60, 1, Qualifier::AllObjects).encode(out);
                Ok(())
            }
        }
    }

    /// Decodes one object body after its header.
    pub fn decode(header: ObjectHeader, input: &mut Reader<'_>) -> Result<Self> {
        match header.group {
            0 => {
                skip_object_body(header.qualifier, input)?;
                Ok(Self::BinaryInputs(Vec::new()))
            }
            1 | 2 => Ok(Self::BinaryInputs(decode_binary_group(
                header.group,
                header.variation,
                header.qualifier,
                input,
            )?)),
            3 | 4 => Ok(Self::DoubleBitBinaryInputs(decode_double_bit_group(
                header.group,
                header.variation,
                header.qualifier,
                input,
            )?)),
            10 => Ok(Self::BinaryOutputs(if header.variation == 1 {
                decode_binary_output_packed(header.qualifier, input)?
            } else {
                decode_binary_like(header.qualifier, input)?
            })),
            20 | 21 | 22 => Ok(Self::Counters(decode_counter_points(
                header.group,
                header.variation,
                header.qualifier,
                input,
            )?)),
            30 | 32 => Ok(Self::AnalogInputs(decode_analog_points(
                header.group,
                header.variation,
                header.qualifier,
                input,
            )?)),
            40 | 42 => {
                let points = decode_analog_points(
                    header.group,
                    header.variation,
                    header.qualifier,
                    input,
                )?;
                Ok(Self::AnalogOutputs(
                    points.into_iter().map(Into::into).collect(),
                ))
            }
            50 if header.variation == 1 => {
                if header.qualifier.count()? != 1 {
                    return Err(ProtocolError::InvalidLength {
                        expected: 1,
                        actual: header.qualifier.count()?,
                    });
                }
                Ok(Self::Time(Timestamp48::decode(input)?))
            }
            60 if header.variation == 1 => Ok(Self::IntegrityPoll),
            60 => Ok(Self::ClassScan(DataClass::parse(header.variation)?)),
            group => Err(ProtocolError::UnknownValue {
                field: "object group",
                value: group,
            }),
        }
    }
}

impl From<BinaryInput> for BinaryOutput {
    fn from(value: BinaryInput) -> Self {
        Self {
            index: value.index,
            flags: value.flags,
        }
    }
}

impl From<BinaryOutput> for BinaryInput {
    fn from(value: BinaryOutput) -> Self {
        Self {
            index: value.index,
            flags: value.flags,
        }
    }
}

impl From<AnalogInput> for AnalogOutput {
    fn from(value: AnalogInput) -> Self {
        Self {
            index: value.index,
            flags: value.flags,
            value: value.value,
        }
    }
}

impl From<AnalogOutput> for AnalogInput {
    fn from(value: AnalogOutput) -> Self {
        Self {
            index: value.index,
            flags: value.flags,
            value: value.value,
        }
    }
}

fn checked_count(count: usize) -> Result<u8> {
    u8::try_from(count).map_err(|_| ProtocolError::InvalidLength {
        expected: u8::MAX as usize,
        actual: count,
    })
}

fn range16_count(start: u16, stop: u16) -> Result<usize> {
    if stop < start {
        return Err(ProtocolError::InvalidLength {
            expected: usize::from(start),
            actual: usize::from(stop),
        });
    }
    Ok(usize::from(stop - start) + 1)
}

fn contiguous_u16_range(indices: &[u16]) -> Option<(u16, u16)> {
    if indices.is_empty() {
        return None;
    }
    let start = indices[0];
    for (offset, index) in indices.iter().enumerate() {
        if *index != start.wrapping_add(offset as u16) {
            return None;
        }
    }
    Some((start, *indices.last()?))
}

fn encode_binary_like<T>(group: u8, variation: u8, values: &[T], out: &mut Vec<u8>) -> Result<()>
where
    T: Copy + Into<BinaryInput>,
{
    let points: Vec<BinaryInput> = values.iter().map(|value| (*value).into()).collect();
    let indices: Vec<u16> = points.iter().map(|value| value.index).collect();
    if let Some((start, stop)) = contiguous_u16_range(&indices) {
        ObjectHeader::new(group, variation, Qualifier::Range16 { start, stop }).encode(out);
        for value in points {
            out.push(value.flags);
        }
        return Ok(());
    }

    ObjectHeader::new(
        group,
        variation,
        Qualifier::CountAndPrefix16(checked_count(values.len())?),
    )
    .encode(out);
    for value in points {
        out.extend_from_slice(&value.index.to_le_bytes());
        out.push(value.flags);
    }
    Ok(())
}

fn skip_object_body(qualifier: Qualifier, input: &mut Reader<'_>) -> Result<()> {
    match qualifier {
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            for _ in 0..count {
                input.read_u8()?;
            }
        }
        Qualifier::AllObjects => {}
        _ => {
            return Err(ProtocolError::UnknownValue {
                field: "unsupported padding object qualifier",
                value: qualifier.code(),
            });
        }
    }
    Ok(())
}

fn decode_binary_output_packed(qualifier: Qualifier, input: &mut Reader<'_>) -> Result<Vec<BinaryOutput>> {
    match qualifier {
        Qualifier::Indexed32(index) => {
            let flags = input.read_u8()?;
            Ok(vec![BinaryOutput {
                index: u16::try_from(index).unwrap_or(u16::MAX),
                flags,
            }])
        }
        _ => decode_binary_like(qualifier, input),
    }
}

fn is_count_and_prefix16_qualifier(code: u8) -> bool {
    !is_simulator_indexed_qualifier(code)
        && (matches!(code, 0x17 | 0x22 | 0x28) || ((code & 0x0F) == 0x02 && code >> 4 <= 0x0A))
}

fn is_simulator_indexed_qualifier(code: u8) -> bool {
    matches!(
        code,
        0x38 | 0x58 | 0x61 | 0x82 | 0x8f | 0x9f | 0xa2 | 0xad | 0xb2 | 0xd8 | 0xf8
    ) || ((code & 0x0F) == 0x0F && code >> 4 >= 0x8)
        || (code >> 4 >= 0xA && matches!(code & 0x0F, 0x0C | 0x0D | 0x0E))
}

fn is_event_group(group: u8) -> bool {
    matches!(group, 2 | 4 | 22 | 32 | 42)
}

fn packed_binary_flags(byte: u8, index_offset: usize) -> u8 {
    let on = (byte >> index_offset) & 0x01 != 0;
    if on { 0x81 } else { 0x00 }
}

fn decode_binary_packed(
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<BinaryInput>> {
    match qualifier {
        Qualifier::Range16 { start, stop } => {
            let count = range16_count(start, stop)?;
            let mut values = Vec::with_capacity(count);
            let mut remaining = count;
            let mut byte_offset = 0_u16;
            while remaining > 0 {
                let byte = input.read_u8()?;
                for bit in 0..8 {
                    if remaining == 0 {
                        break;
                    }
                    values.push(BinaryInput {
                        index: start.wrapping_add(byte_offset * 8 + bit as u16),
                        flags: packed_binary_flags(byte, bit),
                    });
                    remaining -= 1;
                }
                byte_offset += 1;
            }
            Ok(values)
        }
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            let mut values = Vec::with_capacity(count);
            let mut remaining = count;
            let mut byte_offset = 0_u8;
            while remaining > 0 {
                let byte = input.read_u8()?;
                for bit in 0..8 {
                    if remaining == 0 {
                        break;
                    }
                    values.push(BinaryInput {
                        index: u16::from(start.wrapping_add(byte_offset * 8 + bit as u8)),
                        flags: packed_binary_flags(byte, bit),
                    });
                    remaining -= 1;
                }
                byte_offset += 1;
            }
            Ok(values)
        }
        _ => decode_binary_points(1, 2, qualifier, input),
    }
}

fn decode_binary_group(
    group: u8,
    variation: u8,
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<BinaryInput>> {
    if group == 1 && variation == 1 {
        decode_binary_packed(qualifier, input)
    } else {
        decode_binary_points(group, variation, qualifier, input)
    }
}

fn packed_double_bit_flags(byte: u8, index_offset: usize) -> u8 {
    let bits = (byte >> (index_offset * 2)) & 0x03;
    match bits {
        0b10 => 0x81,
        0b01 => 0x41,
        0b11 => 0xc1,
        _ => 0x01,
    }
}

fn decode_double_bit_packed(
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<BinaryInput>> {
    match qualifier {
        Qualifier::Range16 { start, stop } => {
            let count = range16_count(start, stop)?;
            let mut values = Vec::with_capacity(count);
            let mut remaining = count;
            let mut byte_offset = 0_u16;
            while remaining > 0 {
                let byte = input.read_u8()?;
                for slot in 0..4 {
                    if remaining == 0 {
                        break;
                    }
                    values.push(BinaryInput {
                        index: start.wrapping_add(byte_offset * 4 + slot as u16),
                        flags: packed_double_bit_flags(byte, slot),
                    });
                    remaining -= 1;
                }
                byte_offset += 1;
            }
            Ok(values)
        }
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            let mut values = Vec::with_capacity(count);
            let mut remaining = count;
            let mut byte_offset = 0_u8;
            while remaining > 0 {
                let byte = input.read_u8()?;
                for slot in 0..4 {
                    if remaining == 0 {
                        break;
                    }
                    values.push(BinaryInput {
                        index: u16::from(start.wrapping_add(byte_offset * 4 + slot as u8)),
                        flags: packed_double_bit_flags(byte, slot),
                    });
                    remaining -= 1;
                }
                byte_offset += 1;
            }
            Ok(values)
        }
        _ => decode_binary_points(3, 2, qualifier, input),
    }
}

fn decode_double_bit_group(
    group: u8,
    variation: u8,
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<BinaryInput>> {
    if group == 3 && variation == 1 {
        decode_double_bit_packed(qualifier, input)
    } else {
        decode_binary_points(group, variation, qualifier, input)
    }
}

fn event_suffix_bytes(group: u8, variation: u8) -> usize {
    if !is_event_group(group) {
        return 0;
    }
    match variation {
        1 => 0,
        2 | 3 => 6,
        _ => 0,
    }
}

fn skip_event_suffix(input: &mut Reader<'_>, group: u8, variation: u8) -> Result<()> {
    let suffix = event_suffix_bytes(group, variation);
    if suffix != 0 {
        input.read_exact(suffix)?;
    }
    Ok(())
}

fn decode_binary_points(
    group: u8,
    variation: u8,
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<BinaryInput>> {
    match qualifier {
        Qualifier::Range16 { start, stop } => {
            let count = range16_count(start, stop)?;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(BinaryInput {
                    index: start.wrapping_add(offset as u16),
                    flags: input.read_u8()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(BinaryInput {
                    index: u16::from(start.wrapping_add(offset as u8)),
                    flags: input.read_u8()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Indexed32(index) => {
            let flags = input.read_u8()?;
            skip_event_suffix(input, group, variation)?;
            Ok(vec![BinaryInput {
                index: u16::try_from(index).unwrap_or(u16::MAX),
                flags,
            }])
        }
        Qualifier::Count8(count) => {
            let mut values = Vec::with_capacity(usize::from(count));
            for index in 0..usize::from(count) {
                values.push(BinaryInput {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    flags: input.read_u8()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Count16(count) => {
            let count = usize::from(count);
            let mut values = Vec::with_capacity(count);
            for index in 0..count {
                values.push(BinaryInput {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    flags: input.read_u8()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::CountAndPrefix32(count) => {
            let mut values = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let index = input.read_u32_le()?;
                let flags = input.read_u8()?;
                skip_event_suffix(input, group, variation)?;
                values.push(BinaryInput {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    flags,
                });
            }
            Ok(values)
        }
        _ => {
            let count = qualifier.count()?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(BinaryInput {
                    index: input.read_u16_le()?,
                    flags: input.read_u8()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
    }
}

fn decode_binary_like<T>(qualifier: Qualifier, input: &mut Reader<'_>) -> Result<Vec<T>>
where
    BinaryInput: Into<T>,
{
    decode_binary_points(1, 2, qualifier, input)
        .map(|values| values.into_iter().map(Into::into).collect())
}

fn encode_analog_like<T>(group: u8, variation: u8, values: &[T], out: &mut Vec<u8>) -> Result<()>
where
    T: Copy + Into<AnalogInput>,
{
    let points: Vec<AnalogInput> = values.iter().map(|value| (*value).into()).collect();
    let indices: Vec<u16> = points.iter().map(|value| value.index).collect();
    if let Some((start, stop)) = contiguous_u16_range(&indices) {
        ObjectHeader::new(group, variation, Qualifier::Range16 { start, stop }).encode(out);
        for value in points {
            out.push(value.flags);
            out.extend_from_slice(&value.value.to_le_bytes());
        }
        return Ok(());
    }

    ObjectHeader::new(
        group,
        variation,
        Qualifier::CountAndPrefix16(checked_count(values.len())?),
    )
    .encode(out);
    for value in points {
        out.extend_from_slice(&value.index.to_le_bytes());
        out.push(value.flags);
        out.extend_from_slice(&value.value.to_le_bytes());
    }
    Ok(())
}

fn read_analog_value(input: &mut Reader<'_>, variation: u8) -> Result<i32> {
    match variation {
        2 => Ok(i32::from(input.read_i16_le()?)),
        _ => input.read_i32_le(),
    }
}

fn decode_analog_points(
    group: u8,
    variation: u8,
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<AnalogInput>> {
    match qualifier {
        Qualifier::Range16 { start, stop } => {
            let count = range16_count(start, stop)?;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(AnalogInput {
                    index: start.wrapping_add(offset as u16),
                    flags: input.read_u8()?,
                    value: read_analog_value(input, variation)?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(AnalogInput {
                    index: u16::from(start.wrapping_add(offset as u8)),
                    flags: input.read_u8()?,
                    value: read_analog_value(input, variation)?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Indexed32(index) => {
            let flags = input.read_u8()?;
            let value = read_analog_value(input, variation)?;
            skip_event_suffix(input, group, variation)?;
            Ok(vec![AnalogInput {
                index: u16::try_from(index).unwrap_or(u16::MAX),
                flags,
                value,
            }])
        }
        Qualifier::CountAndPrefix32(count) => {
            let mut values = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let index = input.read_u32_le()?;
                let flags = input.read_u8()?;
                let value = read_analog_value(input, variation)?;
                skip_event_suffix(input, group, variation)?;
                values.push(AnalogInput {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    flags,
                    value,
                });
            }
            Ok(values)
        }
        _ => {
            let count = qualifier.count()?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(AnalogInput {
                    index: input.read_u16_le()?,
                    flags: input.read_u8()?,
                    value: read_analog_value(input, variation)?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
    }
}

fn encode_counter_like(values: &[Counter], out: &mut Vec<u8>) -> Result<()> {
    let indices: Vec<u16> = values.iter().map(|value| value.index).collect();
    if let Some((start, stop)) = contiguous_u16_range(&indices) {
        ObjectHeader::new(20, 1, Qualifier::Range16 { start, stop }).encode(out);
        for value in values {
            out.push(value.flags);
            out.extend_from_slice(&value.value.to_le_bytes());
        }
        return Ok(());
    }

    ObjectHeader::new(
        20,
        1,
        Qualifier::CountAndPrefix16(checked_count(values.len())?),
    )
    .encode(out);
    for value in values {
        out.extend_from_slice(&value.index.to_le_bytes());
        out.push(value.flags);
        out.extend_from_slice(&value.value.to_le_bytes());
    }
    Ok(())
}

fn decode_counter_points(
    group: u8,
    variation: u8,
    qualifier: Qualifier,
    input: &mut Reader<'_>,
) -> Result<Vec<Counter>> {
    match qualifier {
        Qualifier::Range16 { start, stop } => {
            let count = range16_count(start, stop)?;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(Counter {
                    index: start.wrapping_add(offset as u16),
                    flags: input.read_u8()?,
                    value: input.read_u32_le()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Range8 { start, stop } => {
            let count = usize::from(stop.saturating_sub(start)) + 1;
            let mut values = Vec::with_capacity(count);
            for offset in 0..count {
                values.push(Counter {
                    index: u16::from(start.wrapping_add(offset as u8)),
                    flags: input.read_u8()?,
                    value: input.read_u32_le()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
        Qualifier::Indexed32(index) => {
            let flags = input.read_u8()?;
            let value = input.read_u32_le()?;
            skip_event_suffix(input, group, variation)?;
            Ok(vec![Counter {
                index: u16::try_from(index).unwrap_or(u16::MAX),
                flags,
                value,
            }])
        }
        Qualifier::CountAndPrefix32(count) => {
            let mut values = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                let index = input.read_u32_le()?;
                let flags = input.read_u8()?;
                let value = input.read_u32_le()?;
                skip_event_suffix(input, group, variation)?;
                values.push(Counter {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    flags,
                    value,
                });
            }
            Ok(values)
        }
        _ => {
            let count = qualifier.count()?;
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(Counter {
                    index: input.read_u16_le()?,
                    flags: input.read_u8()?,
                    value: input.read_u32_le()?,
                });
                skip_event_suffix(input, group, variation)?;
            }
            Ok(values)
        }
    }
}

/// One application-layer fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationFragment {
    /// Application control field.
    pub control: AppControl,
    /// Function code.
    pub function: FunctionCode,
    /// Internal indication bits included in outstation responses.
    pub iin: u16,
    /// Object payloads.
    pub objects: Vec<AppObject>,
}

impl ApplicationFragment {
    /// Creates a new application fragment.
    pub fn new(control: AppControl, function: FunctionCode, objects: Vec<AppObject>) -> Self {
        Self {
            control,
            function,
            iin: 0,
            objects,
        }
    }

    /// Creates an integrity poll request.
    pub fn integrity_poll(sequence: u8) -> Self {
        Self::new(
            AppControl::single(sequence),
            FunctionCode::Read,
            vec![AppObject::IntegrityPoll],
        )
    }

    /// Creates a class scan request matching common Windows simulator clients.
    pub fn simulator_class_scan(sequence: u8) -> Self {
        Self::new(
            AppControl::single(sequence),
            FunctionCode::Read,
            vec![
                AppObject::IntegrityPoll,
                AppObject::ClassScan(DataClass::Class1),
                AppObject::ClassScan(DataClass::Class2),
                AppObject::ClassScan(DataClass::Class3),
            ],
        )
    }

    /// Creates a follow-up class scan without the class-0 integrity object.
    pub fn simulator_event_scan(sequence: u8) -> Self {
        Self::new(
            AppControl::single(sequence),
            FunctionCode::Read,
            vec![
                AppObject::ClassScan(DataClass::Class1),
                AppObject::ClassScan(DataClass::Class2),
                AppObject::ClassScan(DataClass::Class3),
            ],
        )
    }

    fn includes_iin(function: FunctionCode) -> bool {
        matches!(
            function,
            FunctionCode::Response | FunctionCode::UnsolicitedResponse
        )
    }

    /// Encodes the fragment.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.push(self.control.encode());
        out.push(self.function.encode());
        if Self::includes_iin(self.function) {
            out.extend_from_slice(&self.iin.to_le_bytes());
        }
        for object in &self.objects {
            object.encode(&mut out)?;
        }
        Ok(out)
    }

    /// Decodes a fragment.
    pub fn decode(input: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(input);
        let control = AppControl::decode(reader.read_u8()?);
        let function = FunctionCode::decode(reader.read_u8()?)?;
        let iin = if Self::includes_iin(function) {
            reader.read_u16_le()?
        } else {
            0
        };
        let mut objects = Vec::new();
        while reader.remaining() >= 3 {
            reader.skip_padding_zeros();
            if reader.remaining() < 3 {
                break;
            }
            let header = ObjectHeader::decode(&mut reader)?;
            objects.push(AppObject::decode(header, &mut reader)?);
        }
        Ok(Self {
            control,
            function,
            iin,
            objects,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn function_code_parsing() {
        assert_eq!(FunctionCode::decode(1).unwrap(), FunctionCode::Read);
        assert_eq!(FunctionCode::decode(129).unwrap(), FunctionCode::Response);
        assert!(FunctionCode::decode(255).is_err());
    }

    #[test]
    fn object_header_parsing() {
        let header = ObjectHeader::new(1, 2, Qualifier::CountAndPrefix16(3));
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        let mut reader = Reader::new(&encoded);
        assert_eq!(ObjectHeader::decode(&mut reader).unwrap(), header);
    }

    #[test]
    fn contiguous_binary_inputs_use_range16_qualifier() {
        let mut encoded = Vec::new();
        AppObject::BinaryInputs(vec![BinaryInput {
            index: 0,
            flags: 0x81,
        }])
        .encode(&mut encoded)
        .unwrap();
        assert_eq!(
            &encoded,
            &[0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x81]
        );
    }

    #[test]
    fn binary_input_encoding_decoding() {
        round_trip_object(AppObject::BinaryInputs(vec![
            BinaryInput {
                index: 0,
                flags: 0x81,
            },
            BinaryInput {
                index: 3,
                flags: 0x01,
            },
        ]));
    }

    #[test]
    fn binary_output_encoding_decoding() {
        round_trip_object(AppObject::BinaryOutputs(vec![BinaryOutput {
            index: 4,
            flags: 0x81,
        }]));
    }

    #[test]
    fn analog_input_encoding_decoding() {
        round_trip_object(AppObject::AnalogInputs(vec![AnalogInput {
            index: 7,
            flags: 0x81,
            value: -1234,
        }]));
    }

    #[test]
    fn analog_output_encoding_decoding() {
        round_trip_object(AppObject::AnalogOutputs(vec![AnalogOutput {
            index: 8,
            flags: 0x81,
            value: 4321,
        }]));
    }

    #[test]
    fn counter_encoding_decoding() {
        round_trip_object(AppObject::Counters(vec![Counter {
            index: 2,
            flags: 0x81,
            value: 123_456,
        }]));
    }

    #[test]
    fn time_encoding_decoding() {
        round_trip_object(AppObject::Time(
            Timestamp48::from_millis(1_704_067_200_123).unwrap(),
        ));
    }

    #[test]
    fn integrity_poll_round_trip() {
        let fragment = ApplicationFragment::integrity_poll(4);
        let encoded = fragment.encode().unwrap();
        assert_eq!(ApplicationFragment::decode(&encoded).unwrap(), fragment);
    }

    #[test]
    fn class_scan_round_trip() {
        round_trip_object(AppObject::ClassScan(DataClass::Class1));
    }

    #[test]
    fn simulator_double_bit_event_with_22_qualifier_decodes() {
        let payload = &[
            0xc0, 0x81, 0x00, 0x00, 0x04, 0x02, 0x22, 0x01, 0x00, 0x00, 0x81, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00,
        ];
        let response =
            ApplicationFragment::decode(payload).expect("0x22 double-bit event should decode");
        match &response.objects[0] {
            AppObject::DoubleBitBinaryInputs(points) => {
                assert_eq!(points[0].index, 0);
                assert_eq!(points[0].flags, 0x81);
                assert_eq!(double_bit_state(points[0].flags), "ON");
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    fn double_bit_state(flags: u8) -> &'static str {
        match (flags >> 6) & 0x03 {
            0b10 => "ON",
            0b01 => "OFF",
            0b11 => "INDETERMINATE",
            _ => "INTERMEDIATE",
        }
    }

    #[test]
    fn simulator_event_response_with_a2_qualifier_decodes() {
        let payload = &[
            0xc0, 0x81, 0x00, 0x00, 0x02, 0x03, 0xa2, 0x01, 0x00, 0x00, 0x00, 0x00, 0x42, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let response = ApplicationFragment::decode(payload).expect("a2 event should decode");
        match &response.objects[0] {
            AppObject::BinaryInputs(points) => {
                assert_eq!(points[0].flags, 0x42);
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    #[test]
    fn simulator_binary_event_with_ad_qualifier_decodes() {
        let payload = &[
            0xc0, 0x81, 0x00, 0x00, 0x02, 0x03, 0xad, 0x01, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let response =
            ApplicationFragment::decode(payload).expect("0xad binary event should decode");
        match &response.objects[0] {
            AppObject::BinaryInputs(points) => {
                assert_eq!(points[0].flags, 0x81);
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    #[test]
    fn simulator_analog_event_with_8f_qualifier_decodes() {
        let payload = &[
            0xc0, 0x81, 0x00, 0x00, 0x20, 0x03, 0x8f, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let response =
            ApplicationFragment::decode(payload).expect("0x8f analog event should decode");
        match &response.objects[0] {
            AppObject::AnalogInputs(points) => {
                assert_eq!(points[0].index, 0);
                assert_eq!(points[0].value, 0x3f800000_u32 as i32);
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    #[test]
    fn simulator_event_response_with_d8_qualifier_decodes() {
        let payload = &[
            0xc0, 0x81, 0x00, 0x00, 0x02, 0x03, 0xd8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x81, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let response = ApplicationFragment::decode(payload).expect("event response should decode");
        assert_eq!(response.objects.len(), 1);
        match &response.objects[0] {
            AppObject::BinaryInputs(points) => {
                assert_eq!(points.len(), 1);
                assert_eq!(points[0].index, 0);
                assert_eq!(points[0].flags, 0x81);
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    #[test]
    fn freyr_outstation_response_decodes() {
        let payload = &[
            0xc0, 0x81, 0x90, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x01, 0x03, 0x02, 0x00, 0x00,
            0x00, 0x51, 0x0a, 0x01, 0x14, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x15, 0x01, 0x00, 0x00, 0x00, 0x73, 0x7e, 0x01, 0x00, 0x00, 0x00,
        ];
        let response = ApplicationFragment::decode(payload).expect("freyr response should decode");
        match &response.objects[0] {
            AppObject::BinaryInputs(points) => assert_eq!(points[0].flags, 0x01),
            other => panic!("unexpected object {other:?}"),
        }
        match &response.objects[1] {
            AppObject::DoubleBitBinaryInputs(points) => {
                assert_eq!(points[0].flags, 0x51);
                assert_eq!(double_bit_state(points[0].flags), "OFF");
            }
            other => panic!("unexpected object {other:?}"),
        }
    }

    #[test]
    fn freyr_double_bit_flags_use_bits_6_and_7() {
        assert_eq!(double_bit_state(0x81), "ON");
        assert_eq!(double_bit_state(0x41), "OFF");
        assert_eq!(double_bit_state(0x51), "OFF");
    }

    fn round_trip_object(object: AppObject) {
        let fragment =
            ApplicationFragment::new(AppControl::single(1), FunctionCode::Response, vec![object]);
        let encoded = fragment.encode().unwrap();
        assert_eq!(ApplicationFragment::decode(&encoded).unwrap(), fragment);
    }

    proptest! {
        #[test]
        fn binary_inputs_round_trip(values in proptest::collection::vec((any::<u16>(), any::<u8>()), 0..=32)) {
            let values: Vec<BinaryInput> = values
                .into_iter()
                .map(|(index, flags)| BinaryInput { index, flags })
                .collect();
            let fragment = ApplicationFragment::new(
                AppControl::single(2),
                FunctionCode::Response,
                vec![AppObject::BinaryInputs(values)],
            );
            let encoded = fragment.encode().unwrap();
            prop_assert_eq!(ApplicationFragment::decode(&encoded).unwrap(), fragment);
        }
    }
}
