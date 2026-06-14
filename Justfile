#! /usr/bin/env -S just --justfile

# Recipes to check correctness
check: cargo-check clippy-check fmt-check taplo-check deno-check
cargo-check:
    RUSTFLAGS="-Dwarnings" cargo check

clippy-check:
    cargo clippy -- -W clippy::pedantic -W clippy::correctness -W clippy::suspicious -W clippy::cargo

fmt-check:
    cargo fmt --check

taplo-check:
    taplo fmt --check

deno-check:
    deno fmt --check

# Automatic fixup recipes
fix: cargo-fix clippy-fix fmt-fix taplo-fmt deno-fmt
cargo-fix:
    cargo fix --allow-dirty

clippy-fix:
    cargo clippy --fix --allow-dirty -- -W clippy::pedantic -W clippy::correctness -W clippy::suspicious -W clippy::cargo

fmt-fix:
    cargo fmt

taplo-fmt:
    taplo fmt

deno-fmt:
    deno fmt .

test:
    cargo nextest run
