#!/usr/bin/env bash
# The vendored uq-theme watcher is exactly uQ's release (no local edits).
set -eu
cd "$(dirname "$0")/../.."
scripts/check-uq-theme.sh 1.0.0 apps/kyko/src/uq_theme.rs
echo "uq_theme_pin ok"
