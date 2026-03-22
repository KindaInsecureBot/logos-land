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

## How It Works

### Hex Coordinate System

Uses **axial coordinates** `(q, r)` with `u64` type (for PDA seed compatibility).

```
     (q-1,r-1) (q,r-1) (q+1,r-1)
   (q-1, r)  (q, r)  (q+1, r)
     (q-1,r+1) (q,r+1) (q+1,r+1)
```

Every hex has exactly 6 neighbors. The grid is infinite — starts at `(0, 0)` and expands in all directions.

### Privacy Model

Each hex's owner is stored on-chain, but when using **private accounts**, the ownership data lives inside encrypted account state. The owner can then generate zero-knowledge proofs about their land portfolio without revealing:

- Which specific hexes they own
- How many hexes they own in total
- The shape or location of their territory

Only the specific claim being attested (e.g., "I own ≥10 connected tiles") is publicly verifiable.

## Instructions

### `claim(q, r)`

Claim an unclaimed hex tile. The signer becomes the owner. Each hex is a unique PDA derived from its coordinates, so a hex can only be claimed once.

### `transfer(q, r, new_owner)`

Transfer ownership of a hex tile. Only the current owner can transfer. Enables land trading and sales.

### `attest_ownership(q, r)`

Prove you own a specific hex tile. When run as a privacy-preserving transaction, the proof confirms ownership without revealing your identity.

### `attest_connected(min_count)`

Prove you own at least `min_count` **connected** hex tiles. The program runs BFS inside the zkVM to find connected components and asserts the largest ≥ `min_count`. Pass hex accounts as trailing `--hexes` arguments.

### `attest_islands(min_count)`

Prove you own at least `min_count` **islands** (separate connected components). Uses the same BFS algorithm but counts components instead of measuring the largest.

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

### 2. `lez-cli pda` computes wrong addresses for u64 seeds

**Problem**: The `pda` subcommand treats `u64` arg seeds as UTF-8 strings instead of big-endian u64 bytes. The PDA from `pda hex --q 0 --r 0` differs from the PDA actually used during `claim --q 0 --r 0`.

**Workaround**: Run the sequencer with `RUST_LOG=debug` and extract real PDAs from the `account_id:` fields in the log output after claim transactions.

### 3. Manual serialization required (no borsh_derive)

The `borsh_derive` proc macro doesn't compile for the `riscv32im` guest target. `HexTile` uses manual 48-byte serialization: `owner[32] || q[8] || r[8]` (little-endian).

## Architecture

```
land_registry_core/     — Shared types (HexTile struct, manual serialization)
methods/guest/          — On-chain program (zkVM guest binary)
methods/                — risc0-build integration
examples/               — IDL generator + CLI wrapper
```

### Data Layout

`HexTile` — 48 bytes per hex account:
```
[0..32]  owner    — account ID of the owner ([u8; 32])
[32..40] q        — axial coordinate q (u64 LE)
[40..48] r        — axial coordinate r (u64 LE)
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
| 6002 | Not the owner |
| 6003 | Insufficient connected tiles |
| 6004 | Insufficient islands |
| 6005 | Owner mismatch in provided tiles |

## License

MIT
