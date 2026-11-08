#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
SYSROOT="${VESPER_SYSROOT:-$HOME/.cache/vesper-sysroot/root}"
export PATH="$HOME/.cargo/bin:$SYSROOT/usr/bin:${PATH}"
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
export PKG_CONFIG_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig:$SYSROOT/usr/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu:${LD_LIBRARY_PATH:-}"
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
cd "$ROOT"
if [ -s "$HOME/.nvm/nvm.sh" ]; then
  # shellcheck disable=SC1090
  . "$HOME/.nvm/nvm.sh"
  nvm use 20 >/dev/null
fi
exec npm run tauri dev
