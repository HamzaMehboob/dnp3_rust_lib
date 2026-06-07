#!/usr/bin/env sh
set -eu

if ! command -v cargo-tarpaulin >/dev/null 2>&1; then
  cargo install cargo-tarpaulin
fi

cargo tarpaulin --workspace --all-features --out Html --output-dir coverage
