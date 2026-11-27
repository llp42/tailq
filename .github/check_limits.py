#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails unless every test CI runs is limited: python3 -I .github/check_limits.py.

check_workflows.py reads a run line as a whole, so it took a cargo test that shares a line with a build, `cargo
test --no-run && cargo test`, for the build, and one that shares a line with limit.sh, `.github/limit.sh true &&
cargo test`, for a limited run; it never ran cargo bench through limit.sh either; and nothing read limit.sh itself,
which could lose its memory limit or its timeout; nor did anything see lint.yml stop running the checks. Review 5
found each. This checks every command of every run line on its own, runs limit.sh to see that it still limits memory
to 512 MiB and time to LIMIT_SECONDS, and checks that lint.yml runs both scripts.
"""

import pathlib
import re
import shlex
import subprocess
import sys

import yaml

GITHUB = pathlib.Path(__file__).resolve().parent
LIMIT = GITHUB / "limit.sh"
# A cargo command that runs tests or benches: cargo test, cargo bench, cargo miri test, with any toolchain.
RUNS_TESTS = re.compile(r"\bcargo\s+(\+\S+\s+)?(miri\s+)?(test|bench)\b")
# What separates the commands of a line, outside quotes.
SEPARATORS = {"&&", "||", ";", "|", "&"}


def commands(line: str) -> list[list[str]]:
    """The commands of a shell line, each as its words, split at &&, ||, ;, | and &."""
    lexer = shlex.shlex(line, posix=True, punctuation_chars=";&|")
    lexer.whitespace_split = True
    # A # starts no comment here: ${#TITLE} is a length, and a comment's words run nothing of cargo's.
    lexer.commenters = ""
    parts: list[list[str]] = [[]]
    try:
        for word in lexer:
            if word in SEPARATORS:
                parts.append([])
            else:
                parts[-1].append(word)
    except ValueError:
        # An unbalanced quote: the line continues on the next one, which a later check sees on its own.
        pass
    return [part for part in parts if part]


def unlimited(words: list[str]) -> bool:
    """Whether a command runs tests without .github/limit.sh in front of its cargo."""
    while words and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", words[0]):
        words = words[1:]
    limited = bool(words) and words[0] == ".github/limit.sh"
    rest = " ".join(words[1:] if limited else words)
    return bool(RUNS_TESTS.search(rest)) and "--no-run" not in words and not limited


def check_commands() -> list[str]:
    problems = []
    for path in sorted((GITHUB / "workflows").glob("*.yml")):
        for name, job in yaml.safe_load(path.read_text())["jobs"].items():
            for step in job.get("steps", []):
                for line in step.get("run", "").splitlines():
                    for words in commands(line):
                        if unlimited(words):
                            problems.append(f"{path.name}: job {name} runs tests without .github/limit.sh: "
                                            f"{' '.join(words)}")
    return problems


def run(*command: str, seconds: str = "600") -> subprocess.CompletedProcess[str]:
    return subprocess.run([str(LIMIT), *command], capture_output=True, text=True, check=False, timeout=60,
                          env={"PATH": "/usr/bin:/bin", "LIMIT_SECONDS": seconds})


def check_limit_script() -> list[str]:
    problems = []
    data = run("bash", "-c", "ulimit -d").stdout.strip()
    if not data.isdigit() or int(data) > 512 * 1024:
        problems.append(f"limit.sh no longer limits data memory to 512 MiB: ulimit -d is {data or 'unset'}")
    # Only under a limit that reads right, which fails the allocation at once: under none it would take a GiB.
    elif run(sys.executable, "-I", "-c", f"bytearray({1 << 30})").returncode == 0:
        problems.append("limit.sh let a command allocate 1 GiB")
    if run("sleep", "30", seconds="1").returncode != 124:
        problems.append("limit.sh no longer times a command out after LIMIT_SECONDS")
    return problems


def check_lint_job() -> list[str]:
    """lint.yml still runs the checks and their self-tests: a job that lost them would pass with them."""
    runs = [
        step.get("run", "")
        for job in yaml.safe_load((GITHUB / "workflows/lint.yml").read_text())["jobs"].values()
        for step in job.get("steps", [])
    ]
    return [
        f"lint.yml no longer runs {script}"
        for script in (
            "check_workflows.py",
            "check_limits.py",
            "check_workflows_test.py",
            "check_limits_test.py",
            "check_runs.py",
        )
        if not any(f"python3 -I .github/{script}" in run for run in runs)
    ]


def main() -> int:
    problems = check_commands() + check_limit_script() + check_lint_job()
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
