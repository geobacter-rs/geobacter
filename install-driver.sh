#!/usr/bin/env bash

ORIGINAL_RUSTC=$(readlink -f `rustup which rustc`)
UNMODIFIED_RUSTC=$(dirname ${ORIGINAL_RUSTC})/unmodified-rustc

START_DIR=`pwd`
FRAMEWORK_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null && pwd )"

# If our wrapper is already installed, uninstall first.
if [ -e ${UNMODIFIED_RUSTC} ]
then
  mv ${UNMODIFIED_RUSTC} ${ORIGINAL_RUSTC}
fi

cd ${FRAMEWORK_DIR}/rustc-driver
cargo build --release || exit $?
cd ${START_DIR}

# install the driver. We move the original to `unmodified-rustc` to
# avoid having to set LD_LIBRARY_PATH.

mv ${ORIGINAL_RUSTC} ${UNMODIFIED_RUSTC}
cp "${FRAMEWORK_DIR}/target/release/hsa-rustc" ${ORIGINAL_RUSTC}
