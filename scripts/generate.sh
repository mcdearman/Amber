#!/bin/sh
# Regenerate src/Cases.mw for insta.
#
#   scripts/generate.sh
#
# The crate version is pinned in scripts/generate/Cargo.toml. To move to a new
# one, change the pin and run this: if insta has changed,
# the generator stops and says so, and src/ has to be brought into
# line with it before the fingerprint in scripts/generate/src/main.rs is moved.
#
# Needs a Rust toolchain, and `meadow` to format and test the result.

set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"

cargo run --quiet --release --manifest-path "$root/scripts/generate/Cargo.toml" -- "$root"

if command -v meadow >/dev/null 2>&1; then
    (cd "$root" && meadow fmt src && meadow test)
else
    echo "note: meadow is not on PATH, so the result was not formatted or tested" >&2
fi
