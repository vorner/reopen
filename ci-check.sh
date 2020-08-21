#!/bin/sh

# We try to support some older versions of rustc. However, the support is
# tiered a bit. Our dev-dependencies do *not* guarantee that old minimal
# version. So we don't do tests on the older ones. Also, the
# signal-hook-registry supports older rustc than we signal-hook.

set -ex

export PATH="$PATH":~/.cargo/bin
export RUST_BACKTRACE=1
export CARGO_INCREMENTAL=1

rm -f Cargo.lock
cargo build --all --all-features

if [ "$TRAVIS_RUST_VERSION" = 1.31.0 ] ; then
	exit
fi

# Allow some warnings on the very old compiler.
export RUSTFLAGS="-D warnings"

cargo build --all --all-features
cargo test --all --all-features
cargo test --all
cargo doc --no-deps

# Sometimes nightly doesn't have clippy or rustfmt, so don't try that there.
if [ "$TRAVIS_RUST_VERSION" = nightly ] ; then
	exit
fi

cargo clippy --all --tests -- --deny clippy::all
cargo fmt -- --check
