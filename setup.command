#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  echo 'Rust toolchain required. Install from https://rustup.rs' >&2
  exit 1
fi

echo 'Building release binary…'
cargo build --release

BIN_DIR="${HOME}/.local/bin"
mkdir -p "${BIN_DIR}"
install -m 0755 target/release/finder-icon-themes "${BIN_DIR}/finder-icon-themes"

echo 'Removing the build tree to keep this folder small…'
rm -rf target

case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "Note: ${BIN_DIR} is not on your PATH; use the full path below." ;;
esac

cat <<EOF

Installed: ${BIN_DIR}/finder-icon-themes

Try a read-only preview:
  ${BIN_DIR}/finder-icon-themes preview ~/Code/your-repo

Watch every repository under a root and install the background service:
  ${BIN_DIR}/finder-icon-themes service install
EOF
