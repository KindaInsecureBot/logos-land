use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// A single hex tile on the infinite hex grid.
/// Stored as account data for each claimed hex PDA.
#[derive(Debug, Clone, Default, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct HexTile {
    /// Owner's account ID (32 bytes)
    pub owner: [u8; 32],
    /// Axial coordinate q
    pub q: i32,
    /// Axial coordinate r
    pub r: i32,
}
