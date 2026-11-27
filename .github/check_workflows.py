#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails if a workflow loses what the CI limits rest on: python3 -I .github/check_workflows.py.

Every test binary runs under .github/limit.sh, the runner .cargo/config.toml names, and tests/limits.rs fails when
one does not. Miri runs its own runner instead, so each cargo miri test must go through limit.sh itself; and every
job needs a timeout-minutes. It also feeds coverage.py the exports it must fail, as coverage.py merges the
instantiations of generic code, which llvm-cov's own summary counts apart.
"""

import json
import pathlib
import re
import subprocess
import sys
import tempfile

import yaml

GITHUB = pathlib.Path(__file__).resolve().parent
ROOT = GITHUB.parent
# A Miri run of the tests, with any toolchain; a build with --no-run needs no limit.
MIRI_TESTS = re.compile(r"\bcargo\s+(\+\S+\s+)?miri\s+test\b")


def check_workflows() -> list[str]:
    problems = []
    for path in sorted((GITHUB / "workflows").glob("*.yml")):
        for name, job in yaml.safe_load(path.read_text())["jobs"].items():
            where = f"{path.name}: job {name}"
            if "timeout-minutes" not in job:
                problems.append(f"{where} has no timeout-minutes")
            for step in job.get("steps", []):
                for line in step.get("run", "").splitlines():
                    if MIRI_TESTS.search(line) and "--no-run" not in line and ".github/limit.sh" not in line:
                        problems.append(f"{where} runs Miri without .github/limit.sh: {line.strip()}")
    return problems


def coverage(regions: list[tuple[str, int]]) -> int:
    """Runs coverage.py on an export of one function per (file, count), all at the same place."""
    functions = [{"filenames": [name], "regions": [[1, 1, 1, 9, count, 0, 0, 0]]} for name, count in regions]
    with tempfile.TemporaryFile("w+") as export:
        json.dump({"data": [{"functions": functions}]}, export)
        export.seek(0)
        return subprocess.run(
            [sys.executable, "-I", str(GITHUB / "coverage.py")], stdin=export, capture_output=True, check=False
        ).returncode


def check_coverage_script() -> list[str]:
    src = str(ROOT / "src/lib.rs")
    # A checkout under a directory named src: its tests are not the crate's src/.
    outside = "/home/user/src/tailq/tests/model.rs"
    cases = [
        ([(src, 1), (outside, 0)], 0, "counts a region outside the checkout's src/"),
        ([(src, 0), (src, 3)], 0, "does not merge the instantiations of a region"),
        ([(src, 3), (src, 0)], 0, "does not merge the instantiations of a region"),
        ([(src, 0)], 1, "passes a region that never ran"),
        ([(outside, 1)], 1, "passes an export with no region of src/"),
    ]
    return [f"coverage.py {what}" for regions, want, what in cases if coverage(regions) != want]


def main() -> int:
    problems = check_workflows() + check_coverage_script()
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
