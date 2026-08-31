#!/bin/sh
set -eu

repository_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repository_dir"

cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
./scripts/smoke.sh
