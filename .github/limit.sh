#!/bin/bash
# SPDX-License-Identifier: MIT
# Runs a test with at most 512 MiB of data memory and a timeout, so that a test that runs away fails alone instead of
# starving the runner. .cargo/config.toml names it as the runner of every test binary, which cargo builds first,
# without the limit; Miri, which has its own runner, runs under it as a whole. LIMIT_SECONDS sets the timeout, 600 by
# default, and tests/limits.rs fails above that.
set -euo pipefail
ulimit -d $((512 * 1024))
exec timeout --kill-after=10 "${LIMIT_SECONDS:-600}" "$@"
