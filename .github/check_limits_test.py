#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails unless check_limits.py still flags what review 5 found: python3 -I .github/check_limits_test.py.

check_limits.py passes on workflows that keep their limits whether or not it would flag one that lost them, so
review 6 found nothing that would catch it taking a limit.sh earlier on a line for the test's own again, or missing
cargo bench, or reading a line whole, or passing a limit.sh that limits nothing, or a lint.yml that dropped it. This
feeds it each case, as check_workflows.py feeds coverage.py its own.
"""

import importlib.util
import pathlib
import sys
import tempfile

# Loading check_limits.py must leave no __pycache__ in the checkout.
sys.dont_write_bytecode = True
GITHUB = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("check_limits", GITHUB / "check_limits.py")
assert SPEC is not None and SPEC.loader is not None
check_limits = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_limits)

# A run line, and whether one of its commands runs tests without limit.sh.
LINES = [
    ("cargo test --locked", True),
    ("cargo test --locked --no-run && cargo test --locked", True),
    (".github/limit.sh true && cargo test --locked", True),
    (".github/limit.sh true; cargo +nightly-2026-10-04 miri test --locked", True),
    ("cargo bench -p tailq-bench", True),
    ("FLAGS=1 cargo test --locked | tee log", True),
    (".github/limit.sh cargo test --locked", False),
    ('MIRIFLAGS="$FLAGS" .github/limit.sh cargo miri test --locked $ARGS', False),
    ("cargo test --locked --no-run", False),
    ("cargo build --locked --lib && cargo package --locked -p tailq", False),
]

# A limit.sh, and how many of check_limit_script's problems it must raise.
LIMITS = [
    ('exec timeout --kill-after=10 "${LIMIT_SECONDS:-600}" "$@"\n', 1),
    ('ulimit -d $((1024 * 1024))\nexec timeout --kill-after=10 "${LIMIT_SECONDS:-600}" "$@"\n', 1),
    # The one case that waits out check_limits.py's 30-second sleep.
    ('ulimit -d $((512 * 1024))\nexec "$@"\n', 1),
]


def check_lines() -> list[str]:
    return [
        f"check_limits.py {'passes' if want else 'flags'} {line!r}"
        for line, want in LINES
        if any(check_limits.unlimited(words) for words in check_limits.commands(line)) != want
    ]


def check_limit_scripts(scratch: pathlib.Path) -> list[str]:
    problems = []
    real = check_limits.LIMIT
    try:
        for body, want in LIMITS:
            fake = scratch / "limit.sh"
            fake.write_text(f"#!/bin/bash\nset -euo pipefail\n{body}")
            fake.chmod(0o755)
            check_limits.LIMIT = fake
            if len(check_limits.check_limit_script()) != want:
                problems.append(f"check_limits.py does not raise {want} problems with limit.sh {body!r}")
    finally:
        check_limits.LIMIT = real
    return problems


def check_lint_jobs(scratch: pathlib.Path) -> list[str]:
    real = check_limits.GITHUB
    (scratch / "workflows").mkdir()
    (scratch / "workflows/lint.yml").write_text(
        "jobs:\n  workflows:\n    steps:\n      - run: python3 -I .github/check_workflows.py\n"
    )
    try:
        check_limits.GITHUB = scratch
        found = check_limits.check_lint_job()
    finally:
        check_limits.GITHUB = real
    want = [
        f"lint.yml no longer runs {script}"
        for script in ("check_limits.py", "check_workflows_test.py", "check_limits_test.py", "check_runs.py")
    ]
    return [] if found == want else [f"check_limits.py found {found}"]


def main() -> int:
    with tempfile.TemporaryDirectory() as scratch:
        problems = check_lines() + check_limit_scripts(pathlib.Path(scratch)) + check_lint_jobs(pathlib.Path(scratch))
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
