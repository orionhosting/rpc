#!/usr/bin/env bash
export RUSTFLAGS="--remap-path-prefix=$HOME=/build"
cargo build --release "$@"