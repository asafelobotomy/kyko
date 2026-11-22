#!/usr/bin/env bash
# The vendored uq-diag is exactly uQ's release, and the problem codes kyko logs agree with
# uq/diag/kyko.toml (every logged code is listed; every listed code is still logged or retired).
set -eu
cd "$(dirname "$0")/../.."
scripts/check-uq-diag.sh 1.0.1 crates/kyko-diag/src/uq_diag.rs
scripts/check-diag-codes.sh uq/diag/kyko.toml apps crates
echo "uq_diag_pin ok"
