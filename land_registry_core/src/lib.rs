use serde::{Deserialize, Serialize};

/// A single hex tile on the infinite hex grid.
/// Stored as account data for each claimed hex PDA.
///
/// Layout (40 bytes): owner [u8; 32] || q i32 LE || r i32 LE
///
/// Manual serialization avoids borsh_derive proc macro issues
/// in the RISC Zero riscv32im guest target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HexTile {
    /// Owner's account ID (32 bytes)
    pub owner: [u8; 32],
    /// Axial coordinate q
    pub q: i32,
    /// Axial coordinate r
    pub r: i32,
}

impl HexTile {
    /// Fixed serialized size in bytes.
    pub const SIZE: usize = 32 + 4 + 4; // owner + q + r

    /// Serialize to bytes (little-endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.owner);
        buf.extend_from_slice(&self.q.to_le_bytes());
        buf.extend_from_slice(&self.r.to_le_bytes());
        buf
    }

    /// Deserialize from bytes (little-endian).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        let mut owner = [0u8; 32];
        owner.copy_from_slice(&data[..32]);
        let q = i32::from_le_bytes(data[32..36].try_into().ok()?);
        let r = i32::from_le_bytes(data[36..40].try_into().ok()?);
        Some(HexTile { owner, q, r })
    }
}
