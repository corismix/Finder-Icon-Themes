#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
if ! command -v node >/dev/null 2>&1; then
  echo 'Node.js 20 or newer is required.' >&2; exit 1
fi
if ! command -v swiftc >/dev/null 2>&1; then
  echo 'Swift compiler is required: xcode-select --install' >&2; exit 1
fi
npm install
node src/migrate.mjs
printf '\nReady. Try a read-only preview:\n  %s/finder-icon-themes preview ~/Code/AI/Eidolon\n\n' "$PWD"
