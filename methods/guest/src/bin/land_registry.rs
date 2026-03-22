#![no_main]

use std::collections::{HashSet, VecDeque};

use nssa_core::account::AccountWithMetadata;
use nssa_core::program::AccountPostState;
use lez_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

// ---------------------------------------------------------------------------
// Hex grid helpers
// ---------------------------------------------------------------------------

/// Returns the 6 axial neighbors of a hex at (q, r).
/// Uses i64 signed coordinates with standard integer arithmetic.
fn hex_neighbors(q: i64, r: i64) -> [(i64, i64); 6] {
    [
        (q + 1, r),
        (q - 1, r),
        (q, r + 1),
        (q, r - 1),
        (q + 1, r - 1),
        (q - 1, r + 1),
    ]
}

/// Find all connected components in a set of hex tiles.
fn find_connected_components(tiles: &[(i64, i64)]) -> Vec<Vec<(i64, i64)>> {
    let tile_set: HashSet<(i64, i64)> = tiles.iter().copied().collect();
    let mut visited: HashSet<(i64, i64)> = HashSet::new();
    let mut components: Vec<Vec<(i64, i64)>> = Vec::new();

    for &tile in tiles {
        if visited.contains(&tile) {
            continue;
        }

        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(tile);
        visited.insert(tile);

        while let Some((q, r)) = queue.pop_front() {
            component.push((q, r));
            for neighbor in hex_neighbors(q, r) {
                if tile_set.contains(&neighbor) && !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }

        components.push(component);
    }

    components
}

/// Deserialize a HexTile from account data.
fn read_tile(data: &[u8], account_index: usize) -> Result<land_registry_core::HexTile, LezError> {
    land_registry_core::HexTile::from_bytes(data).ok_or(LezError::DeserializationError {
        account_index,
        message: "Invalid HexTile data".to_string(),
    })
}

/// Serialize a HexTile into an account clone.
fn write_tile(
    tile: &land_registry_core::HexTile,
    base: &nssa_core::account::Account,
) -> nssa_core::account::Account {
    let mut updated = base.clone();
    updated.data = tile.to_bytes().try_into().unwrap();
    updated
}

// ---------------------------------------------------------------------------
// LEZ Program
// ---------------------------------------------------------------------------

#[lez_program]
mod land_registry {
    #[allow(unused_imports)]
    use super::*;

    /// Claim an unclaimed hex tile at coordinates (q, r).
    #[instruction]
    pub fn claim(
        #[account(init, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: u64,
        r: u64,
    ) -> LezResult {
        let q_signed = land_registry_core::from_pda_seed(q);
        let r_signed = land_registry_core::from_pda_seed(r);
        let tile = land_registry_core::HexTile {
            owner: *owner.account_id.value(),
            q: q_signed,
            r: r_signed,
            properties: land_registry_core::compute_hex_properties(q_signed, r_signed),
        };

        let new_hex = write_tile(&tile, &hex.account);

        Ok(LezOutput::states_only(vec![
            AccountPostState::new_claimed(new_hex),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Transfer ownership of a hex tile to a new owner.
    #[instruction]
    pub fn transfer(
        #[account(mut, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: u64,
        r: u64,
        new_owner: [u8; 32],
    ) -> LezResult {
        let mut tile = read_tile(&hex.account.data, 0)?;

        if tile.owner != *owner.account_id.value() {
            return Err(LezError::Custom {
                code: 6002,
                message: "Not the owner".to_string(),
            });
        }

        tile.owner = new_owner;
        let updated = write_tile(&tile, &hex.account);

        Ok(LezOutput::states_only(vec![
            AccountPostState::new(updated),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Attest ownership of a specific hex tile.
    #[instruction]
    pub fn attest_ownership(
        #[account(pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: u64,
        r: u64,
    ) -> LezResult {
        let tile = read_tile(&hex.account.data, 0)?;

        if tile.owner != *owner.account_id.value() {
            return Err(LezError::Custom {
                code: 6002,
                message: "Not the owner".to_string(),
            });
        }

        Ok(LezOutput::states_only(vec![
            AccountPostState::new(hex.account.clone()),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Attest that you own at least `min_count` connected hex tiles.
    #[instruction]
    pub fn attest_connected(
        #[account(signer)]
        owner: AccountWithMetadata,
        hexes: Vec<AccountWithMetadata>,
        min_count: u32,
    ) -> LezResult {
        let mut tiles: Vec<(i64, i64)> = Vec::new();
        let owner_id = *owner.account_id.value();

        for (i, hex) in hexes.iter().enumerate() {
            let tile = read_tile(&hex.account.data, i + 1)?;

            if tile.owner != owner_id {
                return Err(LezError::Custom {
                    code: 6005,
                    message: format!("Owner mismatch at hex ({}, {})", tile.q, tile.r),
                });
            }

            tiles.push((tile.q, tile.r));
        }

        let components = find_connected_components(&tiles);
        let largest = components.iter().map(|c| c.len()).max().unwrap_or(0);

        if (largest as u32) < min_count {
            return Err(LezError::Custom {
                code: 6003,
                message: format!(
                    "Insufficient connected tiles: largest component has {}, need {}",
                    largest, min_count
                ),
            });
        }

        let mut post_states = vec![AccountPostState::new(owner.account.clone())];
        for hex in &hexes {
            post_states.push(AccountPostState::new(hex.account.clone()));
        }
        Ok(LezOutput::states_only(post_states))
    }

    /// Attest that you own at least `min_count` separate islands.
    #[instruction]
    pub fn attest_islands(
        #[account(signer)]
        owner: AccountWithMetadata,
        hexes: Vec<AccountWithMetadata>,
        min_count: u32,
    ) -> LezResult {
        let mut tiles: Vec<(i64, i64)> = Vec::new();
        let owner_id = *owner.account_id.value();

        for (i, hex) in hexes.iter().enumerate() {
            let tile = read_tile(&hex.account.data, i + 1)?;

            if tile.owner != owner_id {
                return Err(LezError::Custom {
                    code: 6005,
                    message: format!("Owner mismatch at hex ({}, {})", tile.q, tile.r),
                });
            }

            tiles.push((tile.q, tile.r));
        }

        let components = find_connected_components(&tiles);
        let island_count = components.len() as u32;

        if island_count < min_count {
            return Err(LezError::Custom {
                code: 6004,
                message: format!(
                    "Insufficient islands: found {}, need {}",
                    island_count, min_count
                ),
            });
        }

        let mut post_states = vec![AccountPostState::new(owner.account.clone())];
        for hex in &hexes {
            post_states.push(AccountPostState::new(hex.account.clone()));
        }
        Ok(LezOutput::states_only(post_states))
    }
}
