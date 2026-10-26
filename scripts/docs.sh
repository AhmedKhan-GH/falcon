#!/bin/sh
# Builds the rustdoc API reference into the book and serves it with live reload.
set -e
cd "$(dirname "$0")/.."
cargo doc --workspace --no-deps
rm -rf docs/src/api
cp -R target/doc docs/src/api
# cargo install puts mdbook in ~/.cargo/bin, which may not be on PATH.
MDBOOK=$(command -v mdbook || echo "$HOME/.cargo/bin/mdbook")
exec "$MDBOOK" serve docs "$@"
