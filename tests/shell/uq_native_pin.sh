#!/usr/bin/env bash
# uQ's kits (uq/native.toml) are exactly uQ's releases, and the problem codes kyko logs agree with
# uq/diag/kyko.toml (every logged code is listed; every listed code is still logged or retired).
set -eu
cd "$(dirname "$0")/../.."
scripts/check-uq-native.sh
echo "uq_native_pin ok"
