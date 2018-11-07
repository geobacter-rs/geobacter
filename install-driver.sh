#!/usr/bin/env bash

ORIGINAL_RUSTC=$(readlink -f `rustup which rustc`)
UNMODIFIED_RUSTC=$(dirname ${ORIGINAL_RUSTC})/unmodified-rustc

START_DIR=`pwd`
FRAMEWORK_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null && pwd )"

# If our wrapper is already installed, use it instead.
if [ -e ${UNMODIFIED_RUSTC} ]
then
  mv ${UNMODIFIED_RUSTC} ${ORIGINAL_RUSTC}
fi
export RUSTFLAGS="${RUSTFLAGS} -Z always-encode-mir -Z always-emit-metadata"
cargo build --release --manifest-path ${FRAMEWORK_DIR}/bootstrap-rustc-driver/Cargo.toml || exit $?
# copy to the sysroot so rustc picks up the normal sysroot crates
cp "${FRAMEWORK_DIR}/target/release/bootstrap-rustc-driver" "$(dirname ${ORIGINAL_RUSTC})/bootstrap-rustc"
chrpath -r '$ORIGIN/../lib' "$(dirname ${ORIGINAL_RUSTC})/bootstrap-rustc"
export RUSTC_WRAPPER="$(dirname ${ORIGINAL_RUSTC})/bootstrap-rustc"
cargo build --release --manifest-path ${FRAMEWORK_DIR}/rustc-driver/Cargo.toml || exit $?
chrpath -r '$ORIGIN/../lib' "${FRAMEWORK_DIR}/target/release/legionella-rustc-driver"
# install the driver. We move the original to `unmodified-rustc` to
# avoid having to set LD_LIBRARY_PATH.

mv ${ORIGINAL_RUSTC} ${UNMODIFIED_RUSTC}
cp "${FRAMEWORK_DIR}/target/release/legionella-rustc-driver" ${ORIGINAL_RUSTC}
# check that it runs:
${ORIGINAL_RUSTC} --version || exit $?
