# logos-land

Hex-based land ownership with private attestations on LEZ/NSSA.

An infinite hexagonal grid where anyone can claim tiles. Ownership is **private by default** — nobody can see who owns what unless the owner chooses to prove it. Built on [SPEL](https://github.com/logos-co/spel) / [LEZ](https://github.com/logos-blockchain/lssa).

## Status

✅ **All 5 instructions tested end-to-end on local sequencer:**

| Instruction | Status | Details |
|-------------|--------|---------|
| `claim` | ✅ | Create hex tiles at arbitrary coordinates |
| `transfer` | ✅ | Transfer ownership to new account |
| `attest_ownership` | ✅ | Prove ownership of specific hex (ZK proof) |
| `attest_connected` | ✅ | Prove connected territory of N+ tiles (BFS in zkVM) |
| `attest_islands` | ✅ | Prove N+ separate islands (graph components in zkVM) |

## Privacy Requirement: All Transactions Must Be Privacy-Preserving

> **Warning**: Running with public transactions is only safe for local testing and development. Never use public transactions on a live network.

This program's ownership model relies entirely on the signer identity being hidden. The `owner_hash` commitment (`SHA-256("owner" || pubkey)`) provides zero protection in a public transaction: an observer sees the signer pubkey directly in the transaction, and can trivially recompute `SHA-256("owner" || signer)` to link every tile and `PlayerState` account to a known identity.

**Required: NSSA Privacy-Preserving (PP) transactions**

The PP circuit wraps the same program binary automatically:

1. The signer is hidden behind a nullifier key — the raw pubkey never appears on-chain.
2. Program logic executes inside the zkVM — all ownership checks happen in the zero-knowledge environment.
3. Only encrypted post-states and commitments are published to the chain.
4. The `owner_hash` stored in `HexTile` and `PlayerState` cannot be linked to any external identity by an observer.

The result: the *existence* of claimed hexes is public (an accepted tradeoff for efficient lookups), but *who* owns each hex remains private even to an adversary with full chain history.

## How It Works

### Hex Coordinate System

Uses **axial coordinates** `(q, r)` with `i64` signed integers, supporting negative and positive positions.

```
     (q-1,r-1) (q,r-1) (q+1,r-1)
   (q-1, r)  (q, r)  (q+1, r)
     (q-1,r+1) (q,r+1) (q+1,r+1)
```

Every hex has exactly 6 neighbors. The grid is infinite in all directions — negative and positive.

**PDA seed encoding**: Coordinates are mapped to `u64` PDA seeds via a 2^63 bias offset (`to_pda_seed`/`from_pda_seed`). This preserves ordering: `i64::MIN → 0`, `0 → 2^63`, `i64::MAX → u64::MAX`. The instruction interface takes `q: u64` and `r: u64` (biased) for PDA derivation; internally the program stores signed `i64` values in `HexTile`.

### Privacy Model

**Where is public. Who is private.**

The existence of every claimed hex is visible on-chain (PDA existence is a public fact — an accepted tradeoff for efficient lookups). Hex properties (resource type, terrain value) are also public. But ownership is hidden: no raw pubkey is ever written to account state.

Instead, each `HexTile` stores an `owner_hash`:

```
owner_hash = SHA-256(b"owner" || signer_pubkey)
```

The `"owner"` domain separator prevents rainbow-table attacks against the 32-byte pubkey space. To verify ownership inside the zkVM, the program recomputes `SHA-256(b"owner" || signer)` and compares it to the stored hash — the signer never has to reveal their identity to an external observer.

`PlayerState` PDAs follow the same pattern: the PDA seed is the `owner_hash` rather than the raw pubkey, so even account existence cannot be linked to a known pubkey by an outsider.

The owner can generate zero-knowledge proofs about their land portfolio without revealing:

- Which specific hexes they own
- How many hexes they own in total
- The shape or location of their territory

Only the specific claim being attested (e.g., "I own ≥10 connected tiles") is publicly verifiable.

## Instructions

### `claim(q, r)`

Claim an unclaimed hex tile. The signer becomes the owner. Each hex is a unique PDA derived from its coordinates, so a hex can only be claimed once.

At claim time, **deterministic hex properties** are computed from the coordinates and stored immutably in the tile (see [Hex Properties](#hex-properties) below).

**Two claim modes:**

| Mode | Condition | Requirement |
|------|-----------|-------------|
| Genesis claim | Player has no tiles yet (`tile_count == 0`) | Any unclaimed hex; no adjacency required |
| Expansion claim | Player already owns tiles | Must provide a proof hex that is (a) owned by the signer and (b) a direct axial neighbor of the target hex |

Pass accounts as `extra_accounts`:
- `extra_accounts[0]` — the player's `PlayerState` PDA (`[b"player", owner_hash]` where `owner_hash = SHA-256(b"owner" || signer_pubkey)`; always required)
- `extra_accounts[1]` — adjacent owned proof hex (expansion claims only)

Claiming increments the player's `tile_count` in their `PlayerState`.

### `transfer(q, r, new_owner)`

Transfer ownership of a hex tile. Only the current owner can transfer. Enables land trading and sales.

Both the sender's and receiver's `PlayerState` accounts must be passed:
- `extra_accounts[0]` — sender's `PlayerState` PDA (derived from sender's `owner_hash`; tile_count decremented)
- `extra_accounts[1]` — receiver's `PlayerState` PDA (derived from receiver's `owner_hash`; tile_count incremented; may be uninitialized)

### `attest_ownership(q, r)`

Prove you own a specific hex tile. When run as a privacy-preserving transaction, the proof confirms ownership without revealing your identity.

### `attest_connected(min_count)`

Prove you own at least `min_count` **connected** hex tiles. The program runs BFS inside the zkVM to find connected components and asserts the largest ≥ `min_count`. Pass hex accounts as trailing `--hexes` arguments.

### `attest_islands(min_count)`

Prove you own at least `min_count` **islands** (separate connected components). Uses the same BFS algorithm but counts components instead of measuring the largest.

## Hex Properties

Each hex tile has deterministic properties computed from its coordinates at claim time. The properties are **immutable** — they can never change after a tile is claimed.

### Hash Derivation

```
SHA-256(b"hex_properties" || q.to_be_bytes() || r.to_be_bytes())
```

Where `q` and `r` are the **signed i64 coordinates** (not the biased PDA seeds). This means anyone can compute what a hex "contains" just from its coordinates — no randomness, no on-chain oracle, fully deterministic.

### Derived Fields

| Field | Source | Description |
|-------|--------|-------------|
| `resource_hash` | full hash | Raw 32-byte SHA-256 output — source for all derived properties |
| `resource_type` | `hash[0]` | Resource type (0–255); meaning TBD |
| `terrain_value` | `(hash[1] << 8) \| hash[2]` | Endowment/richness score (0–65535) |

### Example

For tile at `(q=0, r=0)`:
```
SHA-256(b"hex_properties" || [0,0,0,0,0,0,0,0] || [0,0,0,0,0,0,0,0])
```

Anyone can reproduce this with standard SHA-256 — no special tooling required.

## Building

### Prerequisites

- Rust nightly toolchain
- [rzup](https://risczero.com/install) (RISC Zero toolchain manager)
- `RISC0_DEV_MODE=1` for development builds

### Build

```bash
# Set up risc0 toolchain
R="$HOME/.risc0/toolchains/v1.91.1-rust-x86_64-unknown-linux-gnu"
export PATH="$R/bin:$PATH"
export RISC0_DEV_MODE=1

# Build (with OpenSSL from nix if needed)
cargo build --release -j 2

# Generate IDL
cargo run --release --bin generate_idl > land-registry-idl.json
```

### Deploy & Test

```bash
# Start local sequencer (always clean state)
cd ~/lssa
rm -rf rocksdb/
export NSSA_WALLET_HOME_DIR=~/lssa/wallet/configs/debug
./target/release/sequencer_runner sequencer_runner/configs/debug &

# Deploy program
./target/release/wallet deploy-program /path/to/land_registry.bin

# Use a PRECONFIGURED genesis account as signer (NOT a derived account!)
# See "Known Issues" below for why.
SIGNER="6iArKUXxhUJqS7kCaPNhwMWt3ro71PDyBj7jwAyE2VQV"
BINARY="target/riscv-guest/land-registry-methods/land-registry-guest/riscv32im-risc0-zkvm-elf/release/land_registry.bin"
IDL="land-registry-idl.json"
CLI="cargo run --release --bin land_registry_cli --"

# Claim hexes
$CLI --idl $IDL -p $BINARY claim --owner $SIGNER --q 0 --r 0
$CLI --idl $IDL -p $BINARY claim --owner $SIGNER --q 1 --r 0
$CLI --idl $IDL -p $BINARY claim --owner $SIGNER --q 0 --r 1

# Attest ownership
$CLI --idl $IDL -p $BINARY attest-ownership --owner $SIGNER --q 0 --r 0

# Attest connected territory (pass REAL PDAs from claim tx logs)
$CLI --idl $IDL -p $BINARY attest-connected \
  --owner $SIGNER \
  --hexes "HEX00_PDA,HEX10_PDA,HEX01_PDA" \
  --min-count 3

# Transfer ownership
$CLI --idl $IDL -p $BINARY transfer --owner $SIGNER --q 0 --r 0 \
  --new-owner "NEW_OWNER_HEX_64_CHARS"
```

## Known Issues & Workarounds

### 1. Signer must be a preconfigured genesis account

**Problem**: Derived accounts (from `wallet account create`) have `program_owner: [0,0,...,0]`. After the first transaction, NSSA bumps the nonce, making the account non-default. On the next transaction, NSSA rule 7 rejects it: "post state has default program_owner but pre state is non-default."

**Fix**: Use preconfigured accounts from `initial_accounts` in the sequencer config. They have non-default `program_owner`.

### 2. `spel pda` computes wrong addresses for u64 seeds

**Problem**: The `pda` subcommand treats `u64` arg seeds as UTF-8 strings instead of big-endian u64 bytes. The PDA from `pda hex --q 0 --r 0` differs from the PDA actually used during `claim --q 0 --r 0`.

**Workaround**: Run the sequencer with `RUST_LOG=debug` and extract real PDAs from the `account_id:` fields in the log output after claim transactions.

### 3. Manual serialization required (no borsh_derive)

The `borsh_derive` proc macro doesn't compile for the `riscv32im` guest target. `HexTile` uses manual 83-byte serialization: `owner_hash[32] || q[8] || r[8] || properties[35]` (big-endian).

## PlayerState

Each player has a `PlayerState` PDA that tracks how many tiles they own.

**PDA derivation**: `[b"player", owner_hash]` where `owner_hash = SHA-256(b"owner" || player_pubkey)`.
Using the hash as the seed means an outside observer cannot correlate a known pubkey to a tile count.

**Layout** (40 bytes): `owner_hash[32] || tile_count[8 BE]`

The `owner_hash` is stored at account creation and verified on every subsequent read. Passing a different player's `PlayerState` or a fabricated account will be rejected with error 6020.

| Event | Effect on tile_count |
|-------|---------------------|
| Genesis claim | Initialized to 1 |
| Expansion claim | Incremented by 1 |
| Transfer (sender) | Decremented by 1 |
| Transfer (receiver) | Incremented by 1 (initializes if new) |

### Claim Cost (placeholder)

A `claim_cost(tile_count)` function is defined but **not yet enforced**. It returns `tile_count + 1` cost units as a placeholder for future token-based charging. Cost enforcement will be added once a token mechanism is available.

## Architecture

```
land_registry_core/     — Shared types (HexTile, PlayerState — manual serialization)
methods/guest/          — On-chain program (zkVM guest binary)
methods/                — risc0-build integration
examples/               — IDL generator + CLI wrapper
```

### Data Layout

`HexTile` — 83 bytes per hex account:
```
[0..32]  owner_hash     — SHA-256("owner" || owner_pubkey) ([u8; 32])
[32..40] q              — axial coordinate q (i64 BE, signed)
[40..48] r              — axial coordinate r (i64 BE, signed)
[48..80] resource_hash  — SHA-256 of coordinates ([u8; 32])
[80]     resource_type  — resource_hash[0]
[81..83] terrain_value  — (resource_hash[1] << 8 | resource_hash[2]) (u16 BE)
```

`PlayerState` — 40 bytes per player account:
```
[0..32]  owner_hash     — SHA-256("owner" || player_pubkey) ([u8; 32])
[32..40] tile_count     — number of tiles owned (u64 BE)
```

### Graph Algorithms (inside zkVM)

BFS on the hex grid to find connected components:
1. Build a HashSet of all provided tile coordinates
2. For each unvisited tile, BFS through hex neighbors
3. Each BFS produces one connected component
4. Use components for connectivity/island assertions

Hex grids have bounded degree (6), so operations are efficient even inside the zkVM.

## Error Codes

| Code | Meaning |
|------|---------|
| 6002 | Not the owner (or proof hex not owned by signer) |
| 6003 | Insufficient connected tiles |
| 6004 | Insufficient islands |
| 6005 | Owner mismatch in provided tiles |
| 6010 | Missing player_state account in claim |
| 6011 | Expansion claim missing adjacent proof hex |
| 6012 | Proof hex is not adjacent to target hex |
| 6013 | Transfer missing sender/receiver player_state accounts |
| 6020 | PlayerState owner_hash does not match signer (wrong or fabricated account) |
| 6021 | Proof hex has default owner_hash (tile is not claimed) |
| 6022 | Hex tile has default owner_hash (tile is not claimed) |

## License

MIT
