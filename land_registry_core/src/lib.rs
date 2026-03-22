use sha2::{Digest, Sha256};
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

/// Deterministic properties for a hex tile, derived from its coordinates via SHA-256.
///
/// Hash derivation: `SHA-256(b"hex_properties" || q.to_be_bytes() || r.to_be_bytes())`
///
/// Properties are immutable — computed once at claim time from signed i64 coordinates.
///
/// Layout (35 bytes): resource_hash[32] || resource_type[1] || terrain_value[2 BE]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexProperties {
    /// Full SHA-256 output — raw material for deriving any future property.
    pub resource_hash: [u8; 32],
    /// Resource type: `resource_hash[0]` (interpretation TBD).
    pub resource_type: u8,
    /// Terrain value (endowment/richness score 0–65535):
    /// `(resource_hash[1] as u16) << 8 | resource_hash[2] as u16`
    pub terrain_value: u16,
}

impl Default for HexProperties {
    fn default() -> Self {
        Self {
            resource_hash: [0u8; 32],
            resource_type: 0,
            terrain_value: 0,
        }
    }
}

impl HexProperties {
    /// Fixed serialized size in bytes.
    pub const SIZE: usize = 32 + 1 + 2; // hash + type + value

    /// Serialize to bytes (big-endian for multi-byte fields).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.resource_hash);
        buf.push(self.resource_type);
        buf.extend_from_slice(&self.terrain_value.to_be_bytes());
        buf
    }

    /// Deserialize from bytes (big-endian for multi-byte fields).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        let mut resource_hash = [0u8; 32];
        resource_hash.copy_from_slice(&data[..32]);
        let resource_type = data[32];
        let terrain_value = u16::from_be_bytes(data[33..35].try_into().ok()?);
        Some(HexProperties { resource_hash, resource_type, terrain_value })
    }
}

/// Compute deterministic hex properties from signed coordinates.
///
/// `SHA-256(b"hex_properties" || q.to_be_bytes() || r.to_be_bytes())`
///
/// Anyone can reproduce this computation from coordinates alone — no randomness.
pub fn compute_hex_properties(q: i64, r: i64) -> HexProperties {
    let mut hasher = Sha256::new();
    hasher.update(b"hex_properties");
    hasher.update(q.to_be_bytes());
    hasher.update(r.to_be_bytes());
    let hash: [u8; 32] = hasher.finalize().into();
    HexProperties {
        resource_hash: hash,
        resource_type: hash[0],
        terrain_value: (hash[1] as u16) << 8 | hash[2] as u16,
    }
}

/// Compute the owner commitment hash for a pubkey.
///
/// `SHA-256(b"owner" || pubkey)`
///
/// The `"owner"` domain separator prevents rainbow table attacks.
/// Store this hash in `HexTile::owner_hash` instead of the raw pubkey
/// so that on-chain state reveals *where* territory is but not *who* owns it.
pub fn compute_owner_hash(pubkey: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"owner");
    hasher.update(pubkey);
    hasher.finalize().into()
}

/// Tracks per-player state: how many tiles the player currently owns.
///
/// Layout (40 bytes): owner_hash[32] || tile_count[8 BE]
///
/// PDA derived from player owner_hash: `[b"player", owner_hash]`
/// where `owner_hash = compute_owner_hash(player_pubkey)`.
/// Using the hash as the PDA seed prevents linking a PlayerState account
/// to a raw pubkey by external observers.
///
/// The stored `owner_hash` ties this account to a specific player. On every
/// read the program verifies the stored hash matches the signer, so an attacker
/// cannot substitute a different player's PlayerState or supply a fabricated one.
///
/// Manual serialization — no borsh_derive (not compatible with riscv32im zkVM guest).
#[derive(Debug, Clone)]
pub struct PlayerState {
    /// Owner commitment hash: `SHA-256(b"owner" || player_pubkey)`.
    /// Stored at creation and verified on every subsequent use.
    pub owner_hash: [u8; 32],
    /// Number of tiles currently owned by this player.
    pub tile_count: u64,
}

impl PlayerState {
    /// Fixed serialized size in bytes.
    pub const SIZE: usize = 32 + 8; // owner_hash + tile_count = 40

    /// Serialize to bytes (big-endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.owner_hash);
        buf.extend_from_slice(&self.tile_count.to_be_bytes());
        buf
    }

    /// Deserialize from bytes (big-endian).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        let mut owner_hash = [0u8; 32];
        owner_hash.copy_from_slice(&data[..32]);
        let tile_count = u64::from_be_bytes(data[32..40].try_into().ok()?);
        Some(PlayerState { owner_hash, tile_count })
    }
}

impl Default for PlayerState {
    fn default() -> Self {
        Self { owner_hash: [0u8; 32], tile_count: 0 }
    }
}

/// A single hex tile on the infinite hex grid.
/// Stored as account data for each claimed hex PDA.
///
/// Layout (83 bytes): owner_hash[32] || q[8 BE] || r[8 BE] || properties[35]
///
/// `owner_hash` is `SHA-256(b"owner" || owner_pubkey)` — the raw pubkey is never
/// stored on-chain, so the map of claimed tiles is public but ownership is private.
///
/// Coordinates are i64 (signed), supporting negative and positive grid positions.
/// PDA seeds use a biased u64 encoding via `to_pda_seed`/`from_pda_seed`:
/// bias = 2^63, so coordinate 0 maps to seed 2^63.
///
/// Manual serialization avoids borsh_derive proc macro issues
/// in the RISC Zero riscv32im guest target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HexTile {
    /// SHA-256 commitment to the owner's pubkey: `SHA-256(b"owner" || pubkey)`.
    /// Hides identity while still allowing ownership verification inside the zkVM.
    pub owner_hash: [u8; 32],
    /// Axial coordinate q (signed)
    pub q: i64,
    /// Axial coordinate r (signed)
    pub r: i64,
    /// Deterministic properties derived from coordinates at claim time (immutable)
    pub properties: HexProperties,
}

impl HexTile {
    /// Fixed serialized size in bytes.
    pub const SIZE: usize = 32 + 8 + 8 + HexProperties::SIZE; // owner_hash + q + r + properties = 83

    /// Serialize to bytes (big-endian).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.extend_from_slice(&self.owner_hash);
        buf.extend_from_slice(&self.q.to_be_bytes());
        buf.extend_from_slice(&self.r.to_be_bytes());
        buf.extend_from_slice(&self.properties.to_bytes());
        buf
    }

    /// Deserialize from bytes (big-endian).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        let mut owner_hash = [0u8; 32];
        owner_hash.copy_from_slice(&data[..32]);
        let q = i64::from_be_bytes(data[32..40].try_into().ok()?);
        let r = i64::from_be_bytes(data[40..48].try_into().ok()?);
        let properties = HexProperties::from_bytes(&data[48..])?;
        Some(HexTile { owner_hash, q, r, properties })
    }
}
