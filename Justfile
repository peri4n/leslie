set shell := ["bash", "-cu"]
set positional-arguments

# Build and run N local gossip nodes with shared stdout
# Usage: just simulate [n] [base] [host]

# Build and run N local gossip nodes with shared stdout
# Positional args are used inside the script ($1..$3)
simulate n='3' base='50051' host='127.0.0.1':
	#!/usr/bin/env bash
	set -euo pipefail
	export RUST_LOG=info
	cargo build -q
	bin=target/debug/leslie
	# read args passed by just's positional-arguments
	n="${1:-3}"
	base="${2:-50051}"
	host="${3:-127.0.0.1}"
	# seed node
	stdbuf -oL -eL "$bin" --id node1 --hostname "$host" --port "$base" &
	pids=$!
	# start nodes 2..n and connect to seed
	if [ "$n" -gt 1 ]; then
		for i in $(seq 2 "$n"); do
			port=$((base + i - 1))
			stdbuf -oL -eL "$bin" --id "node${i}" --hostname "$host" --port "$port" --connect "$host:$base" &
			pids="$pids $!"
		done
	fi
	trap 'kill $pids >/dev/null 2>&1 || true' INT TERM EXIT
	wait
