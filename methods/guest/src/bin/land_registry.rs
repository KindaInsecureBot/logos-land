#![no_main]

// PRIVACY REQUIREMENT: This program MUST be executed as a privacy-preserving (PP)
// transaction via NSSA's PP circuit. Public transactions expose the signer pubkey
// directly on-chain, making the owner_hash commitment meaningless — an observer
// can trivially recompute SHA-256("owner" || signer) and link every tile to an
// identity. In PP mode the signer is hidden behind a nullifier key, the program
// logic runs inside the zkVM, and only encrypted post-states are published.
// Public transactions are only acceptable for local testing/development.

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

/// Cost to claim based on the player's current tile count.
/// TODO: implement actual token charging when token mechanism is available.
fn claim_cost(current_tile_count: u64) -> u64 {
    // Placeholder: returns cost units, not yet enforced.
    current_tile_count + 1
}

/// Check if two hexes are axial neighbors (distance == 1).
/// Valid neighbor offsets: (±1,0), (0,±1), (1,−1), (−1,1).
fn are_neighbors(q1: i64, r1: i64, q2: i64, r2: i64) -> bool {
    let dq = q2.wrapping_sub(q1);
    let dr = r2.wrapping_sub(r1);
    matches!((dq, dr), (1, 0) | (-1, 0) | (0, 1) | (0, -1) | (1, -1) | (-1, 1))
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

/// Serialize a PlayerState into an account clone.
fn write_player_state(
    state: &land_registry_core::PlayerState,
    base: &nssa_core::account::Account,
) -> nssa_core::account::Account {
    let mut updated = base.clone();
    updated.data = state.to_bytes().try_into().unwrap();
    updated
}

/// Read a PlayerState from account data, returning a default (tile_count=0) if the account
/// has not been initialized yet (empty or too-short data).
fn read_player_state(
    data: &[u8],
    account_index: usize,
) -> Result<(land_registry_core::PlayerState, bool), LezError> {
    if data.len() < land_registry_core::PlayerState::SIZE {
        // Account not yet initialized — treat as a new player.
        return Ok((land_registry_core::PlayerState::default(), true));
    }
    let state = land_registry_core::PlayerState::from_bytes(data).ok_or(LezError::DeserializationError {
        account_index,
        message: "Invalid PlayerState data".to_string(),
    })?;
    Ok((state, false))
}

// ---------------------------------------------------------------------------
// LEZ Program
// ---------------------------------------------------------------------------

#[lez_program]
mod land_registry {
    #[allow(unused_imports)]
    use super::*;

    /// Claim an unclaimed hex tile at coordinates (q, r).
    ///
    /// Two modes:
    /// - **Genesis claim** (first hex): `extra_accounts` contains only the player's
    ///   `PlayerState` PDA (len == 1). No adjacency required.
    /// - **Expansion claim**: `extra_accounts[0]` is the player's `PlayerState` PDA and
    ///   `extra_accounts[1]` is a proof hex that (a) is owned by the signer and (b) is a
    ///   direct axial neighbor of the target hex (len == 2).
    ///
    /// `extra_accounts[0]` — PlayerState PDA (`[b"player", signer_pubkey]`)
    /// `extra_accounts[1]` — adjacent owned hex (expansion only)
    #[instruction]
    pub fn claim(
        #[account(init, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: u64,
        r: u64,
        extra_accounts: Vec<AccountWithMetadata>,
    ) -> LezResult {
        // extra_accounts[0] = PlayerState PDA (always required)
        if extra_accounts.is_empty() {
            return Err(LezError::Custom {
                code: 6010,
                message: "Missing player_state account (extra_accounts[0])".to_string(),
            });
        }

        let signer_hash = land_registry_core::compute_owner_hash(owner.account_id.value());

        let player_state_account = &extra_accounts[0];
        let (player_state, is_new_player) =
            read_player_state(&player_state_account.account.data, 2)?;

        // Verify that the PlayerState account belongs to this signer.
        // A default (all-zero) owner_hash means the account is uninitialized (new player).
        if !is_new_player && player_state.owner_hash != signer_hash {
            return Err(LezError::Custom {
                code: 6020,
                message: "PlayerState owner_hash does not match signer".to_string(),
            });
        }

        let q_signed = land_registry_core::from_pda_seed(q);
        let r_signed = land_registry_core::from_pda_seed(r);

        let is_genesis = player_state.tile_count == 0 && is_new_player;
        let _cost = claim_cost(player_state.tile_count); // placeholder — not yet enforced

        if !is_genesis {
            // Expansion claim: require adjacent proof hex in extra_accounts[1].
            if extra_accounts.len() < 2 {
                return Err(LezError::Custom {
                    code: 6011,
                    message: "Expansion claim requires adjacent proof hex (extra_accounts[1])"
                        .to_string(),
                });
            }
            let proof_account = &extra_accounts[1];
            let proof_tile = read_tile(&proof_account.account.data, 3)?;

            // Proof hex must be a claimed tile (non-default owner_hash).
            if proof_tile.owner_hash == [0u8; 32] {
                return Err(LezError::Custom {
                    code: 6021,
                    message: "Proof hex is not claimed (default owner_hash)".to_string(),
                });
            }

            // Proof hex must be owned by the signer.
            if proof_tile.owner_hash != signer_hash {
                return Err(LezError::Custom {
                    code: 6002,
                    message: "Proof hex is not owned by signer".to_string(),
                });
            }

            // Proof hex must be a direct neighbor of the target hex.
            if !are_neighbors(q_signed, r_signed, proof_tile.q, proof_tile.r) {
                return Err(LezError::Custom {
                    code: 6012,
                    message: "Proof hex is not adjacent to target hex".to_string(),
                });
            }
        }

        // Build the new tile.
        let tile = land_registry_core::HexTile {
            owner_hash: signer_hash,
            q: q_signed,
            r: r_signed,
            properties: land_registry_core::compute_hex_properties(q_signed, r_signed),
        };
        let new_hex = write_tile(&tile, &hex.account);

        // Increment player tile count; always store owner_hash (new or existing account).
        let updated_player_state = land_registry_core::PlayerState {
            owner_hash: signer_hash,
            tile_count: player_state.tile_count + 1,
        };
        let updated_ps_account =
            write_player_state(&updated_player_state, &player_state_account.account);
        let ps_post = if is_new_player {
            AccountPostState::new_claimed(updated_ps_account)
        } else {
            AccountPostState::new(updated_ps_account)
        };

        let mut post_states = vec![
            AccountPostState::new_claimed(new_hex),
            AccountPostState::new(owner.account.clone()),
            ps_post,
        ];

        // Return proof hex unchanged (framework requires all inputs in output).
        if !is_genesis {
            post_states.push(AccountPostState::new(extra_accounts[1].account.clone()));
        }

        Ok(LezOutput::states_only(post_states))
    }

    /// Transfer ownership of a hex tile to a new owner.
    ///
    /// `extra_accounts[0]` — sender's PlayerState PDA (decremented)
    /// `extra_accounts[1]` — receiver's PlayerState PDA (incremented; may be uninitialized)
    #[instruction]
    pub fn transfer(
        #[account(mut, pda = [literal("hex"), arg("q"), arg("r")])]
        hex: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        q: u64,
        r: u64,
        new_owner: [u8; 32],
        extra_accounts: Vec<AccountWithMetadata>,
    ) -> LezResult {
        if extra_accounts.len() < 2 {
            return Err(LezError::Custom {
                code: 6013,
                message: "Transfer requires sender and receiver player_state accounts \
                           (extra_accounts[0] and extra_accounts[1])"
                    .to_string(),
            });
        }

        let signer_hash = land_registry_core::compute_owner_hash(owner.account_id.value());
        let new_owner_hash = land_registry_core::compute_owner_hash(&new_owner);

        let mut tile = read_tile(&hex.account.data, 0)?;

        if tile.owner_hash != signer_hash {
            return Err(LezError::Custom {
                code: 6002,
                message: "Not the owner".to_string(),
            });
        }

        tile.owner_hash = new_owner_hash;
        let updated_hex = write_tile(&tile, &hex.account);

        // Sender's PlayerState — verify identity, then decrement tile_count.
        let sender_ps_account = &extra_accounts[0];
        let (mut sender_state, sender_is_new) =
            read_player_state(&sender_ps_account.account.data, 2)?;
        if !sender_is_new && sender_state.owner_hash != signer_hash {
            return Err(LezError::Custom {
                code: 6020,
                message: "Sender PlayerState owner_hash does not match signer".to_string(),
            });
        }
        if sender_state.tile_count > 0 {
            sender_state.tile_count -= 1;
        }
        sender_state.owner_hash = signer_hash;
        let updated_sender = write_player_state(&sender_state, &sender_ps_account.account);

        // Receiver's PlayerState — verify identity (if existing), then increment tile_count.
        let receiver_ps_account = &extra_accounts[1];
        let (mut receiver_state, is_new_receiver) =
            read_player_state(&receiver_ps_account.account.data, 3)?;
        if !is_new_receiver && receiver_state.owner_hash != new_owner_hash {
            return Err(LezError::Custom {
                code: 6020,
                message: "Receiver PlayerState owner_hash does not match new_owner".to_string(),
            });
        }
        receiver_state.owner_hash = new_owner_hash;
        receiver_state.tile_count += 1;
        let updated_receiver = write_player_state(&receiver_state, &receiver_ps_account.account);
        let receiver_post = if is_new_receiver {
            AccountPostState::new_claimed(updated_receiver)
        } else {
            AccountPostState::new(updated_receiver)
        };

        Ok(LezOutput::states_only(vec![
            AccountPostState::new(updated_hex),
            AccountPostState::new(owner.account.clone()),
            AccountPostState::new(updated_sender),
            receiver_post,
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

        if tile.owner_hash != land_registry_core::compute_owner_hash(owner.account_id.value()) {
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
        let owner_hash = land_registry_core::compute_owner_hash(owner.account_id.value());

        for (i, hex) in hexes.iter().enumerate() {
            let tile = read_tile(&hex.account.data, i + 1)?;

            // Reject unclaimed (default) tiles.
            if tile.owner_hash == [0u8; 32] {
                return Err(LezError::Custom {
                    code: 6022,
                    message: format!("Tile at index {} is not claimed", i),
                });
            }

            if tile.owner_hash != owner_hash {
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
        let owner_hash = land_registry_core::compute_owner_hash(owner.account_id.value());

        for (i, hex) in hexes.iter().enumerate() {
            let tile = read_tile(&hex.account.data, i + 1)?;

            // Reject unclaimed (default) tiles.
            if tile.owner_hash == [0u8; 32] {
                return Err(LezError::Custom {
                    code: 6022,
                    message: format!("Tile at index {} is not claimed", i),
                });
            }

            if tile.owner_hash != owner_hash {
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
