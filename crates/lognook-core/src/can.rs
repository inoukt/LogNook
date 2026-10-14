//! Classic CAN frames and explicitly selected research fields.
use crate::{LogNookError, Sample, SampleOrigin, SampleQuality};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanDirection {
    Rx,
    Tx,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanByteOrder {
    Intel,
    Motorola,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CanFrame {
    pub elapsed_ns: i64,
    pub wall_time_unix_ns: i64,
    pub can_id: u32,
    pub extended: bool,
    pub direction: CanDirection,
    pub data: Vec<u8>,
    /// Transport flags retained as raw evidence, without inferring signal meaning.
    pub flags: u32,
}

fn validate_id(can_id: u32, extended: bool) -> Result<(), LogNookError> {
    if can_id > if extended { 0x1fff_ffff } else { 0x7ff } {
        return Err(LogNookError::new(
            "CAN identifier exceeds its 11-bit or 29-bit format",
        ));
    }
    Ok(())
}

impl CanFrame {
    pub fn validate(&self) -> Result<(), LogNookError> {
        validate_id(self.can_id, self.extended)?;
        if self.data.len() > 8 {
            return Err(LogNookError::new(
                "Classic CAN payload must contain at most 8 bytes",
            ));
        }
        if self.elapsed_ns < 0 {
            return Err(LogNookError::new(
                "CAN elapsed timestamp must be non-negative",
            ));
        }
        Ok(())
    }

    /// Validate the transport DLC before discarding it in favor of the actual bytes.
    pub fn validate_dlc(&self, dlc: u8) -> Result<(), LogNookError> {
        self.validate()?;
        if dlc > 8 || usize::from(dlc) != self.data.len() {
            return Err(LogNookError::new(
                "Classic CAN DLC must match the payload length (0-8)",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct CanSignalDefinition {
    pub id: String,
    pub name: String,
    pub can_id: u32,
    pub extended: bool,
    /// DBC bit position (LSB0 within a byte). Intel selects the field LSB;
    /// Motorola selects its MSB, walking downward then from bit 0 to bit 15.
    pub start_bit: u8,
    pub bit_length: u8,
    pub byte_order: CanByteOrder,
    pub signed: bool,
    pub factor: f64,
    pub offset: f64,
    pub unit: String,
}

fn next_bit(bit: usize, byte_order: CanByteOrder) -> usize {
    match byte_order {
        CanByteOrder::Intel => bit + 1,
        CanByteOrder::Motorola if bit % 8 == 0 => bit + 15,
        CanByteOrder::Motorola => bit - 1,
    }
}

impl CanSignalDefinition {
    pub fn validate(&self) -> Result<(), LogNookError> {
        validate_id(self.can_id, self.extended)?;
        if !(1..=64).contains(&self.bit_length) {
            return Err(LogNookError::new("CAN signal bit length must be 1-64"));
        }
        if !self.factor.is_finite() || !self.offset.is_finite() {
            return Err(LogNookError::new(
                "CAN signal factor and offset must be finite",
            ));
        }
        let mut bit = usize::from(self.start_bit);
        for _ in 0..self.bit_length {
            if bit >= 64 {
                return Err(LogNookError::new(
                    "CAN signal field exceeds the classic CAN payload",
                ));
            }
            bit = next_bit(bit, self.byte_order);
        }
        Ok(())
    }
}

pub fn decode_can_signal(
    frame: &CanFrame,
    definition: &CanSignalDefinition,
    source_id: &str,
) -> Result<Option<Sample>, LogNookError> {
    frame.validate()?;
    definition.validate()?;
    if frame.direction != CanDirection::Rx
        || frame.can_id != definition.can_id
        || frame.extended != definition.extended
    {
        return Ok(None);
    }
    let mut raw = 0u64;
    let mut bit = usize::from(definition.start_bit);
    for index in 0..definition.bit_length {
        let byte = frame
            .data
            .get(bit / 8)
            .ok_or_else(|| LogNookError::new("CAN frame is too short for the selected signal"))?;
        let value = u64::from((byte >> (bit % 8)) & 1);
        match definition.byte_order {
            CanByteOrder::Intel => raw |= value << index,
            CanByteOrder::Motorola => raw = (raw << 1) | value,
        }
        bit = next_bit(bit, definition.byte_order);
    }
    let numeric = if definition.signed {
        let shift = 64 - definition.bit_length;
        ((raw << shift) as i64 >> shift) as f64
    } else {
        raw as f64
    };
    let value = numeric * definition.factor + definition.offset;
    if !value.is_finite() {
        return Err(LogNookError::new("Scaled CAN signal value must be finite"));
    }
    Ok(Some(Sample {
        elapsed_ns: frame.elapsed_ns,
        wall_time_unix_ns: frame.wall_time_unix_ns,
        source_id: source_id.into(),
        ecu_id: Some(if frame.extended {
            format!("{:08X}", frame.can_id)
        } else {
            format!("{:03X}", frame.can_id)
        }),
        signal_id: definition.id.clone(),
        value,
        unit: definition.unit.clone(),
        // An extracted user field remains a research hypothesis.
        quality: SampleQuality::Suspect,
        origin: SampleOrigin::Live,
        raw_bytes: Some(frame.data.clone()),
    }))
}
