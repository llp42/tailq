#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails if a workflow loses what f9f1d04 and d7aa842 put in: python3 -I .github/check_workflows.py.

Every job has a timeout-minutes; every step that runs tests runs them through .github/limit.sh, after an
earlier step of the same job built them without it (`--no-run`, or, for doctests, a `cargo build` on the same
toolchain of every package they belong to, which review 6 found unchecked); clippy also
runs on release builds, and the bench smoke optimised; and coverage.py keeps only the regions of the
checkout's own src/ and merges instantiations. Review 4 found nothing that would catch any of these coming
back. lint.yml runs it in a job of its own; it reads the workflows with PyYAML, which the runner's python3
has.
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
# A command that runs tests: cargo test or cargo miri test, with any toolchain.
RUNS_TESTS = re.compile(r"\bcargo\s+(\+\S+\s+)?(miri\s+)?test\b")


def command(line: str) -> frozenset[str]:
    """The words of the cargo command on a line, without --no-run, and with the matrix's args as the env names
    them."""
    cargo = line[line.index("cargo") :].replace("${{ matrix.args }}", "$ARGS")
    return frozenset(cargo.split()) - {"--no-run"}


def packages(line: str) -> tuple[str | None, frozenset[str]]:
    """The toolchain of the cargo command on a line, and the packages it names, tailq when it names none."""
    words = line[line.index("cargo") :].split()
    toolchain = next((word for word in words if word.startswith("+")), None)
    named = frozenset(after for word, after in zip(words, words[1:]) if word in {"-p", "--package"}) | frozenset(
        word.removeprefix("--package=").removeprefix("-p")
        for word in words
        if word.startswith("--package=") or (word.startswith("-p") and word != "-p" and not word.startswith("--"))
    )
    return toolchain, named or frozenset({"tailq"})


def check_job(where: str, job: dict) -> list[str]:
    problems = []
    if "timeout-minutes" not in job:
        problems.append(f"{where} has no timeout-minutes")
    # What earlier steps built: the commands built with --no-run, and the toolchain and packages of each cargo
    # build, which is what a run of doctests can build first.
    built: set[frozenset[str]] = set()
    libraries: list[tuple[str | None, frozenset[str]]] = []
    for step in job.get("steps", []):
        for line in step.get("run", "").splitlines():
            if not RUNS_TESTS.search(line):
                if re.search(r"\bcargo\s+(\+\S+\s+)?build\b", line):
                    libraries.append(packages(line))
                continue
            toolchain, needed = packages(line)
            doctests = "--doc" in line.split() and any(
                toolchain == other and needed <= library for other, library in libraries
            )
            if "--no-run" in line:
                built.add(command(line))
            elif ".github/limit.sh" not in line.split("cargo")[0]:
                problems.append(f"{where} runs tests without .github/limit.sh: {line.strip()}")
            elif not (command(line) in built or doctests):
                problems.append(f"{where} runs tests it did not build first: {line.strip()}")
    return problems


def check_job_rules() -> list[str]:
    """Feeds check_job the jobs it must flag and those it must pass."""

    def job(*runs: str) -> dict:
        return {"timeout-minutes": 5, "steps": [{"run": run} for run in runs]}

    doc = ".github/limit.sh cargo test --locked --doc -p tailq -p tailq-flip"
    cases = [
        (job("cargo build --locked --lib -p tailq -p tailq-flip", doc), False, "flags doctests it built"),
        (job("cargo build --locked --lib", doc), True, "passes doctests of a package no build named"),
        (job("cargo +nightly build --locked --lib -p tailq -p tailq-flip", doc), True,
         "passes doctests built on another toolchain"),
        (job("cargo build --locked --lib", ".github/limit.sh cargo test --locked --doc"), False,
         "flags tailq's doctests after its build"),
        (job("cargo test --locked --no-run", "cargo test --locked"), True, "passes tests run without limit.sh"),
        (job(".github/limit.sh cargo test --locked"), True, "passes tests it did not build first"),
        ({"steps": []}, True, "passes a job with no timeout-minutes"),
    ]
    return [f"check_workflows.py {what}" for steps, want, what in cases if bool(check_job("job", steps)) != want]


def check_workflows() -> list[str]:
    problems = []
    for path in sorted((GITHUB / "workflows").glob("*.yml")):
        workflow = yaml.safe_load(path.read_text())
        for name, job in workflow["jobs"].items():
            problems += check_job(f"{path.name}: job {name}", job)
    clippy = [
        step.get("run", "")
        for step in yaml.safe_load((GITHUB / "workflows/lint.yml").read_text())["jobs"]["clippy"]["steps"]
    ]
    if not any("cargo clippy" in run and "--release" in run for run in clippy):
        problems.append("lint.yml: clippy no longer runs on release builds")
    test = (GITHUB / "workflows/test.yml").read_text()
    if not any("tailq-bench" in line and "--release" in line for line in test.splitlines() if "limit.sh" in line):
        problems.append("test.yml: the bench smoke no longer runs optimised")
    if "--export-prefix" in test:
        problems.append("test.yml: the coverage job uses the deprecated show-env --export-prefix")
    return problems


def coverage(regions: list[tuple[str, int]]) -> int:
    """Runs coverage.py on an export of one function per (file, count), all at the same place."""
    functions = [
        {"filenames": [name], "regions": [[1, 1, 1, 9, count, 0, 0, 0]]} for name, count in regions
    ]
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
    problems = check_workflows() + check_job_rules() + check_coverage_script()
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
