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
fn hex_neighbors(q: i32, r: i32) -> [(i32, i32); 6] {
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
/// Returns a vec of components, each component is a vec of (q, r) coords.
fn find_connected_components(tiles: &[(i32, i32)]) -> Vec<Vec<(i32, i32)>> {
    let tile_set: HashSet<(i32, i32)> = tiles.iter().copied().collect();
    let mut visited: HashSet<(i32, i32)> = HashSet::new();
    let mut components: Vec<Vec<(i32, i32)>> = Vec::new();

    for &tile in tiles {
        if visited.contains(&tile) {
            continue;
        }

        // BFS from this tile
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

// ---------------------------------------------------------------------------
// LEZ Program
// ---------------------------------------------------------------------------

#[lez_program]
mod land_registry {
    #[allow(unused_imports)]
    use super::*;

    /// Claim an unclaimed hex tile at coordinates (q, r).
    /// The signer becomes the owner. The hex PDA is derived from the coordinates.
    #[instruction]
    pub fn claim(
        #[account(init, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: i32,
        r: i32,
    ) -> LezResult {
        let tile = land_registry_core::HexTile {
            owner: *owner.account_id.value(),
            q,
            r,
        };

        let data = borsh::to_vec(&tile)
            .map_err(|e| LezError::SerializationError { message: e.to_string() })?;

        let mut new_hex = hex.account.clone();
        new_hex.data = data.try_into().unwrap();

        Ok(LezOutput::states_only(vec![
            AccountPostState::new_claimed(new_hex),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Transfer ownership of a hex tile to a new owner.
    /// Only the current owner (signer) can transfer.
    #[instruction]
    pub fn transfer(
        #[account(mut, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: i32,
        r: i32,
        new_owner: [u8; 32],
    ) -> LezResult {
        let mut tile: land_registry_core::HexTile =
            borsh::from_slice(&hex.account.data)
                .map_err(|e| LezError::DeserializationError {
                    account_index: 0,
                    message: e.to_string(),
                })?;

        // Verify the signer is the current owner
        if tile.owner != *owner.account_id.value() {
            return Err(LezError::Custom {
                code: 6002,
                message: "Not the owner".to_string(),
            });
        }

        tile.owner = new_owner;

        let data = borsh::to_vec(&tile)
            .map_err(|e| LezError::SerializationError { message: e.to_string() })?;

        let mut updated = hex.account.clone();
        updated.data = data.try_into().unwrap();

        Ok(LezOutput::states_only(vec![
            AccountPostState::new(updated),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Attest ownership of a specific hex tile.
    /// When run as a privacy-preserving transaction, proves you own the hex
    /// without revealing your identity. The coordinates are public (in instruction
    /// data), but the owner is hidden inside the zkVM.
    #[instruction]
    pub fn attest_ownership(
        #[account(pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: i32,
        r: i32,
    ) -> LezResult {
        let tile: land_registry_core::HexTile =
            borsh::from_slice(&hex.account.data)
                .map_err(|e| LezError::DeserializationError {
                    account_index: 0,
                    message: e.to_string(),
                })?;

        if tile.owner != *owner.account_id.value() {
            return Err(LezError::Custom {
                code: 6002,
                message: "Not the owner".to_string(),
            });
        }

        // Success — the proof receipt attests ownership
        Ok(LezOutput::states_only(vec![
            AccountPostState::new(hex.account.clone()),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Attest that you own at least `min_count` connected hex tiles.
    ///
    /// Pass your hex tile accounts as trailing accounts. The program verifies
    /// ownership of each, extracts coordinates, runs BFS to find connected
    /// components, and asserts the largest component has >= min_count tiles.
    ///
    /// When run privately: coordinates and ownership stay hidden inside the zkVM.
    /// Only min_count (in instruction data) and the success/failure are visible.
    #[instruction]
    pub fn attest_connected(
        #[account(signer)]
        owner: AccountWithMetadata,
        hexes: Vec<AccountWithMetadata>,
        min_count: u32,
    ) -> LezResult {
        let mut tiles: Vec<(i32, i32)> = Vec::new();
        let owner_id = *owner.account_id.value();

        // Verify ownership and extract coordinates from each hex account
        for (i, hex) in hexes.iter().enumerate() {
            let tile: land_registry_core::HexTile =
                borsh::from_slice(&hex.account.data)
                    .map_err(|e| LezError::DeserializationError {
                        account_index: (i + 1) as u32, // +1 because owner is index 0
                        message: e.to_string(),
                    })?;

            if tile.owner != owner_id {
                return Err(LezError::Custom {
                    code: 6005,
                    message: format!("Owner mismatch at hex ({}, {})", tile.q, tile.r),
                });
            }

            tiles.push((tile.q, tile.r));
        }

        // Find connected components via BFS
        let components = find_connected_components(&tiles);

        // Find the largest connected component
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

        // Return all accounts unchanged — the proof is the attestation
        let mut post_states = vec![AccountPostState::new(owner.account.clone())];
        for hex in &hexes {
            post_states.push(AccountPostState::new(hex.account.clone()));
        }
        Ok(LezOutput::states_only(post_states))
    }

    /// Attest that you own at least `min_count` separate islands (connected components).
    ///
    /// An island is a group of connected hex tiles separated from other groups.
    /// Proves you have land spread across multiple distinct areas without revealing
    /// which hexes you own or where they are.
    #[instruction]
    pub fn attest_islands(
        #[account(signer)]
        owner: AccountWithMetadata,
        hexes: Vec<AccountWithMetadata>,
        min_count: u32,
    ) -> LezResult {
        let mut tiles: Vec<(i32, i32)> = Vec::new();
        let owner_id = *owner.account_id.value();

        // Verify ownership and extract coordinates
        for (i, hex) in hexes.iter().enumerate() {
            let tile: land_registry_core::HexTile =
                borsh::from_slice(&hex.account.data)
                    .map_err(|e| LezError::DeserializationError {
                        account_index: (i + 1) as u32,
                        message: e.to_string(),
                    })?;

            if tile.owner != owner_id {
                return Err(LezError::Custom {
                    code: 6005,
                    message: format!("Owner mismatch at hex ({}, {})", tile.q, tile.r),
                });
            }

            tiles.push((tile.q, tile.r));
        }

        // Count connected components (islands)
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

        // Return all accounts unchanged
        let mut post_states = vec![AccountPostState::new(owner.account.clone())];
        for hex in &hexes {
            post_states.push(AccountPostState::new(hex.account.clone()));
        }
        Ok(LezOutput::states_only(post_states))
    }
}
