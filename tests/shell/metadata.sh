#!/usr/bin/env bash
# The packaged metadata is valid (AppStream and the desktop entry).
set -eu
cd "$(dirname "$0")/../.."
if command -v appstreamcli >/dev/null; then appstreamcli validate --no-net packaging/org.kyko.Kyko.metainfo.xml; fi
if command -v desktop-file-validate >/dev/null; then desktop-file-validate packaging/org.kyko.Kyko.desktop; fi
echo "metadata ok"
