#!/usr/bin/env bash
#
# publish-if-new.sh — idempotent `cargo publish` wrapper.
#
# Usage: ./publish-if-new.sh <crate-name>
#
# Reads the workspace version from `Cargo.toml`, queries the crates.io
# API for the crate's currently-published max_version, and runs
# `cargo publish -p <crate>` only when the two differ. When the version
# is already on the registry, the script prints a notice and exits 0,
# leaving the workflow free to continue to the next crate.
#
# Why this exists: the previous release workflow used a flat
# `cargo publish -p X && cargo publish -p Y && cargo publish -p Z`
# chain. When a release got half-published (e.g. rustio-macros made it
# to crates.io but rustio-core's upload failed for a transient reason),
# re-running the workflow at the same tag failed with
# `error: crate rustio-macros@VERSION already exists` — and the
# operator had to either bump versions or publish manually from local.
# This script makes the workflow safely re-runnable from any state.
#
# crates.io requires a User-Agent header on API requests; the curl
# call below sets one. Failures to query the API fall through to a
# real `cargo publish` attempt — better to surface the actual cargo
# error than to silently skip on a connectivity hiccup.

set -euo pipefail

crate="${1:?usage: publish-if-new.sh <crate-name>}"

# Read the workspace version from the root Cargo.toml. Anchored to the
# `[workspace.package]` block so we don't accidentally pick up a
# pinned-dep version line lower in the file.
workspace_version=$(
  awk '
    /^\[workspace\.package\]/ { in_block = 1; next }
    /^\[/                     { in_block = 0 }
    in_block && /^version[[:space:]]*=/ {
      match($0, /"[^"]+"/)
      print substr($0, RSTART + 1, RLENGTH - 2)
      exit
    }
  ' Cargo.toml
)

if [[ -z "$workspace_version" ]]; then
  echo "::error::could not read workspace.package.version from Cargo.toml" >&2
  exit 1
fi

# Query the crates.io API. The endpoint is forgiving: 404 means the
# crate has never been published (so any version is new), any other
# failure → treat the published version as unknown and let cargo
# publish make the final decision.
api_response=$(
  curl -sS -L \
    -A "rustio-release-script/1 (https://github.com/abdulwahed-sweden/rustio)" \
    --max-time 30 \
    "https://crates.io/api/v1/crates/${crate}" || true
)

published_version=$(
  printf '%s' "$api_response" \
    | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    print(data.get('crate', {}).get('max_version', ''))
except Exception:
    pass
" 2>/dev/null || true
)

echo "rustio release: ${crate} workspace=${workspace_version} crates.io=${published_version:-<unknown>}"

if [[ "$published_version" == "$workspace_version" ]]; then
  echo "::notice::${crate}@${workspace_version} is already on crates.io — skipping publish"
  exit 0
fi

cargo publish -p "$crate"
