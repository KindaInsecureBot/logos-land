use serde::{Deserialize, Serialize};

/// Bias offset: 2^63. Maps the i64 signed range to u64 for PDA seeds.
/// i64::MIN → 0, 0 → 2^63, i64::MAX → u64::MAX — ordering is preserved.
const COORD_BIAS: u64 = 1u64 << 63;

/// Convert a signed coordinate to a biased u64 PDA seed.
pub fn to_pda_seed(coord: i64) -> u64 {
    (coord as u64).wrapping_add(COORD_BIAS)
}

/// Convert a biased u64 PDA seed back to a signed coordinate.
pub fn from_pda_seed(seed: u64) -> i64 {
    seed.wrapping_sub(COORD_BIAS) as i64
}

/// A single hex tile on the infinite hex grid.
/// Stored as account data for each claimed hex PDA.
///
/// Layout (48 bytes): owner [u8; 32] || q i64 BE || r i64 BE
///
/// Coordinates are i64 (signed), supporting negative and positive grid positions.
/// PDA seeds use a biased u64 encoding via `to_pda_seed`/`from_pda_seed`:
/// bias = 2^63, so coordinate 0 maps to seed 2^63.
///
/// Manual serialization avoids borsh_derive proc macro issues
/// in the RISC Zero riscv32im guest target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HexTile {
    /// Owner's account ID (32 bytes)
    pub owner: [u8; 32],
    /// Axial coordinate q (signed)
    pub q: i64,
    /// Axial coordinate r (signed)
    pub r: i64,
}

impl HexTile {
    /// Fixed serialized size in bytes.
    pub const SIZE: usize = 32 + 8 + 8; // owner + q + r

    /// Serialize to bytes (big-endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.owner);
        buf.extend_from_slice(&self.q.to_be_bytes());
        buf.extend_from_slice(&self.r.to_be_bytes());
        buf
    }

    /// Deserialize from bytes (big-endian).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        let mut owner = [0u8; 32];
        owner.copy_from_slice(&data[..32]);
        let q = i64::from_be_bytes(data[32..40].try_into().ok()?);
        let r = i64::from_be_bytes(data[40..48].try_into().ok()?);
        Some(HexTile { owner, q, r })
    }
}
