#!/bin/bash
# Cross-compile for Windows (x86_64-pc-windows-gnu)
#
# Prerequisites:
#   sudo apt install mingw-w64 gcc-mingw-w64-x86-64

set -e

# Set OpenSSL include path for libsqlite3-sys (sqlcipher) cross-compilation
# Uses native headers since the API is compatible
export CFLAGS_x86_64_pc_windows_gnu="-I/usr/include"

# Build for Windows
cargo build --target x86_64-pc-windows-gnu "$@"
