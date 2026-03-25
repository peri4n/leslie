# Leslie

Leslie is a tiny gRPC node (named for Leslie Lamport). 
It talks to other Leslie nodes and shares information. 
Useful for learning and experimenting with distributed systems.

## Quick Start

- Build:
  - `cargo build`
- Run a single node:
  - `RUST_LOG=info cargo run -- --id node1 --hostname 127.0.0.1 --port 50051`
- Simulate N local nodes (uses just):
  - `just simulate 3`            # 3 nodes starting at 50051
  - `just simulate 4 50100 127.0.0.1`  # n=4, base port=50100, host=127.0.0.1

Notes:
- Build generates gRPC code from `proto/gossip.proto` via `build.rs`. Ensure `protoc` is available, or use a dev shell that provides it.
- Set `RUST_LOG=info` to see gossip and connection logs.

## CLI

The binary accepts:
- `--id <string>`: node id (default: `foo`)
- `--hostname <host>`: bind host (default: `[::1]`)
- `--port <u16>`: bind port (default: `0`)
- `--connect <addr>`: seed to connect to, e.g. `127.0.0.1:50051`

Example: start a seed and a peer
```
RUST_LOG=info cargo run -- --id node1 --hostname 127.0.0.1 --port 50051
RUST_LOG=info cargo run -- --id node2 --hostname 127.0.0.1 --port 50052 --connect 127.0.0.1:50051
```

## What It Does

- Exposes a gRPC service `Gossip::gossip` that accepts a caller's `(node_id, address)` and replies with a map of known peers.
- Periodically gossips to known peers to merge peer maps.

## Requirements

- Rust toolchain
- `just` (optional, for convenience): https://github.com/casey/just
- `protoc` in PATH for gRPC codegen

## Roadmap

- [ ] Gossip
- [ ] Tracing
- [ ] Heart Bleed
- [ ] Leader election

