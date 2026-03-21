# logos-land

Hex-based land ownership with private attestations on LEZ/NSSA.

An infinite hexagonal grid where anyone can claim tiles. Ownership is **private by default** — nobody can see who owns what unless the owner chooses to prove it. Built on [SPEL](https://github.com/logos-co/spel) / [LEZ](https://github.com/logos-blockchain/lssa).

## How It Works

### Hex Coordinate System

Uses **axial coordinates** `(q, r)` — the standard system for hex grids.

```
     (-1,-1) (0,-1) (1,-1)
   (-1, 0) (0, 0) (1, 0)
     (-1, 1) (0, 1) (1, 1)
```

Every hex has exactly 6 neighbors:
```
(q+1, r)    (q-1, r)
(q, r+1)    (q, r-1)
(q+1, r-1)  (q-1, r+1)
```

The grid is infinite — it starts at the origin `(0, 0)` and expands in all directions.

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

Prove you own a specific hex tile. When run as a privacy-preserving transaction, the proof confirms ownership without revealing your identity — only the coordinates and the validity of the proof are public.

### `attest_connected(min_count)`

Prove you own at least `min_count` **connected** hex tiles. Connected means the tiles share edges (are hex neighbors). The program:

1. Accepts your hex tile accounts as trailing accounts
2. Verifies you own each tile
3. Extracts coordinates and builds an adjacency graph
4. Runs BFS to find connected components
5. Asserts the largest component ≥ `min_count`

When run privately: nobody learns which hexes you own, where your territory is, or its shape. They only learn "this person owns a connected territory of at least N tiles."

### `attest_islands(min_count)`

Prove you own at least `min_count` **islands** (separate connected components). An island is a group of hex tiles connected to each other but separated from other groups.

Uses the same graph algorithm as `attest_connected`, but counts the number of connected components instead of the size of the largest one.

This proves you have land spread across multiple distinct areas — useful for proving geographic diversity without revealing locations.

## Graph Algorithms

The connectivity proofs run **inside the RISC Zero zkVM**. The algorithms use BFS on the hex grid:

1. Build a set of all provided tile coordinates
2. For each unvisited tile, start a BFS
3. Explore neighbors (6 per hex, constant degree)
4. Each BFS produces one connected component
5. Collect all components for island counting or size checking

Hex grids have bounded degree (6), so the graph operations are efficient even inside the zkVM.

## Building

### Prerequisites

- Rust nightly toolchain
- [rzup](https://risczero.com/install) (RISC Zero toolchain manager)

### Build

```bash
# Build the zkVM guest binary
make build

# Generate IDL
make idl

# Deploy to local sequencer
make setup
make deploy

# Claim a hex
make cli ARGS="claim --q 0 --r 0 --owner-account <SIGNER_BASE58>"

# Attest ownership
make cli ARGS="attest-ownership --q 0 --r 0 --owner-account <SIGNER_BASE58>"
```

### Local Sequencer

See [lssa](https://github.com/logos-blockchain/lssa) for running a local sequencer:

```bash
cd ~/lssa
cargo build --release -p sequencer_runner -p wallet --features standalone
rm -rf rocksdb/  # always start clean
./target/release/sequencer_runner sequencer_runner/configs/debug &
```

## Architecture

```
land_registry_core/     — Shared types (HexTile struct)
methods/guest/          — On-chain program (zkVM guest binary)
examples/               — IDL generator + CLI wrapper
```

## Error Codes

| Code | Meaning |
|------|---------|
| 6002 | Not the owner |
| 6003 | Insufficient connected tiles |
| 6004 | Insufficient islands |
| 6005 | Owner mismatch in provided tiles |

## License

MIT
