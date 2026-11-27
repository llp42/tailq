#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails unless check_workflows.py flags what review 7 found, and test.yml keeps the steps 3643e01 added:
python3 -I .github/check_workflows_test.py.

check_job_rules fed check_job no doctests of tailq after a build of another package only, none named with
--package, and no tests other than doctests after a bare cargo build, so review 7's mutants of packages() and of the
doctest rule passed it, as did one of main() that no longer ran check_job_rules. Nor did anything read test.yml for
the steps 3643e01 added: the flip crate's tests, its doctests in the compile-fail job, and the tests of the packaged
crate, which review 6 found could ship a test of a dependency it drops; review 7 deleted each and every check passed,
as it did when lint.yml stopped running check_limits_test.py.
"""

import contextlib
import importlib.util
import io
import pathlib
import sys

import yaml

# Loading check_workflows.py must leave no __pycache__ in the checkout.
sys.dont_write_bytecode = True
GITHUB = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("check_workflows", GITHUB / "check_workflows.py")
assert SPEC is not None and SPEC.loader is not None
check_workflows = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_workflows)


def job(*runs: str) -> dict:
    return {"timeout-minutes": 5, "steps": [{"run": run} for run in runs]}


# A job, whether check_job must flag it, and what it shows.
JOBS = [
    (job("cargo build --locked --lib -p tailq-flip", ".github/limit.sh cargo test --locked --doc"), True,
     "tailq's doctests after a build of tailq-flip alone"),
    (job("cargo build --locked --lib", ".github/limit.sh cargo test --locked --doc --package tailq-flip"), True,
     "tailq-flip's doctests, named with --package, after a build of tailq alone"),
    (job("cargo build --locked --lib --package tailq-flip", ".github/limit.sh cargo test --locked --doc -p tailq-flip"),
     False, "tailq-flip's doctests after a build that names it with --package"),
    (job("cargo build --locked --lib", ".github/limit.sh cargo test --locked"), True,
     "tests other than doctests after a cargo build"),
    (job("cargo build --locked --lib", ".github/limit.sh cargo test --locked --doc --package=tailq-flip"), True,
     "tailq-flip's doctests, named with --package=, after a build of tailq alone"),
    (job("cargo build --locked --lib", ".github/limit.sh cargo test --locked --doc -ptailq-flip"), True,
     "tailq-flip's doctests, named with -ptailq-flip, after a build of tailq alone"),
    (job("cargo build --locked --lib -ptailq-flip", ".github/limit.sh cargo test --locked --doc -p tailq-flip"), False,
     "tailq-flip's doctests after a build that names it as -ptailq-flip"),
]


def check_jobs() -> list[str]:
    return [
        f"check_workflows.py {'passes' if want else 'flags'} {what}"
        for steps, want, what in JOBS
        if bool(check_workflows.check_job("job", steps)) != want
    ]


def check_main() -> list[str]:
    """main() runs check_job_rules: with a check_job that flags nothing, check_job_rules fails, and so must main."""
    real = check_workflows.check_job
    check_workflows.check_job = lambda where, steps: []
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            status = check_workflows.main()
    finally:
        check_workflows.check_job = real
    return [] if status == 1 else ["check_workflows.py's main() no longer runs check_job_rules"]


def limited(step: dict) -> list[str]:
    """The lines of a step that run tests under limit.sh."""
    return [
        line for line in step.get("run", "").splitlines()
        if check_workflows.RUNS_TESTS.search(line) and ".github/limit.sh" in line.split("cargo")[0]
    ]


def check_steps() -> list[str]:
    jobs = yaml.safe_load((GITHUB / "workflows/test.yml").read_text())["jobs"]
    problems = []
    test = [line for step in jobs["test"]["steps"] for line in limited(step)]
    if not any("--doc" not in line.split() and "tailq-flip" in check_workflows.packages(line)[1] for line in test):
        problems.append("test.yml: the test job no longer runs the flip crate's tests")
    doc = [line for step in jobs["compile-fail"]["steps"] for line in limited(step) if "--doc" in line.split()]
    if not any(check_workflows.packages(line)[1] >= {"tailq", "tailq-flip"} for line in doc):
        problems.append("test.yml: the compile-fail job no longer runs the doctests of both packages")
    packaged = [
        step for step in jobs["test"]["steps"]
        if "cargo package" in step.get("run", "")
        and any("--manifest-path" in line and "target/package/" in line for line in limited(step))
    ]
    if not packaged:
        problems.append("test.yml: the test job no longer runs the tests of the packaged crate")
    # check_limits.py's check_lint_job requires only itself and check_workflows.py.
    lint = [
        step.get("run", "")
        for job in yaml.safe_load((GITHUB / "workflows/lint.yml").read_text())["jobs"].values()
        for step in job.get("steps", [])
    ]
    if not any("python3 -I .github/check_limits_test.py" in run for run in lint):
        problems.append("lint.yml no longer runs check_limits_test.py")
    return problems


def main() -> int:
    problems = check_jobs() + check_main() + check_steps()
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
