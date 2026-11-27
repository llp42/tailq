#!/bin/bash
# SPDX-License-Identifier: MIT
# Runs a test command with at most 512 MiB of data memory and a timeout, so that a test that runs away fails
# alone instead of starving the runner. Build first, without the limit: a linker or rustc may need more.
# LIMIT_SECONDS sets the timeout, 600 by default.
set -euo pipefail
ulimit -d $((512 * 1024))
exec timeout --kill-after=10 "${LIMIT_SECONDS:-600}" "$@"
