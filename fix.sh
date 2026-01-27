#! /usr/bin/env bash
set -eux

echo "Running 'cargo fix' on the codebase"
cargo fix --allow-dirty

echo "Running clippy linter and applying available fixes"
cargo clippy --fix --allow-dirty -- -W clippy::pedantic \
  -W clippy::correctness \
  -W clippy::suspicious \
  -W clippy::cargo

echo "Running Rust formatter"
cargo fmt

echo "Running TOML formatter"
taplo fmt
