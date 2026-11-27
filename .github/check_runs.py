#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails if CI could stop running something with every other check passing: python3 -I .github/check_runs.py.

check_workflows.py and check_limits.py check how the tests run, under limit.sh and after a build, and
check_workflows_test.py that test.yml keeps three steps; none read what the jobs run. Review 8 deleted a Miri
matrix entry, the threads job's sixteen seeds, the matrix's args and flags from the Miri runs, the compile-fail
job's nightly toolchain, the test job's --release, the coverage job's coverage.py, a job's failure (with
continue-on-error) and the synchronize trigger, and every check passed; as it did when a checker's main() dropped
one of its rules, or check_workflows_test.py's step checks were weakened. This runs the Miri step as each matrix
entry would, with cargo and limit.sh stubbed, and checks that every configuration the README promises runs; checks
each job's commands in order, that nothing may fail softly, and the triggers; feeds every checker's main() each rule
on its own; and feeds check_workflows_test.py's step checks the steps it must miss. Each rule here is fed the
mutation of the real workflows that it must flag.
"""

import contextlib
import importlib.util
import io
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import yaml

# Loading the other checkers must leave no __pycache__ in the checkout.
sys.dont_write_bytecode = True
GITHUB = pathlib.Path(__file__).resolve().parent
STRICT = "-Zmiri-strict-provenance"

# What the Miri job must run, as the README lists it: a name, the flags each run needs, the flags it must not
# have, its arguments after `cargo miri test --locked`, and how many runs under distinct seeds.
MIRI = [
    ("stacked borrows with symbolic alignment", {STRICT, "-Zmiri-symbolic-alignment-check"}, {"-Zmiri-tree-borrows"},
     [], 1),
    ("tree borrows", {STRICT, "-Zmiri-tree-borrows"}, set(), [], 1),
    ("release", {STRICT}, set(), ["--release"], 1),
    ("i686", {STRICT}, set(), ["--target", "i686-unknown-linux-gnu"], 1),
    ("s390x", {STRICT}, set(), ["--target", "s390x-unknown-linux-gnu"], 1),
    ("the thread tests under sixteen seeds", {STRICT}, set(), ["--test", "raw"], 16),
]

# The commands each job runs, in order, each a whole line of a run step: a job that lost one passes otherwise.
LIMIT = re.escape(".github/limit.sh ")
COMMANDS = {
    ("test.yml", "test"): [
        LIMIT + re.escape("cargo test --locked"),
        LIMIT + re.escape("cargo test --locked --release"),
        LIMIT + re.escape("cargo test --locked -p tailq-flip"),
        LIMIT + re.escape("cargo test --locked -p tailq-bench --benches --release"),
        re.escape("cargo build --locked --target x86_64-unknown-none"),
    ],
    ("test.yml", "compile-fail"): [LIMIT + re.escape("cargo test --locked --doc -p tailq -p tailq-flip")],
    ("test.yml", "coverage"): [
        re.escape("source <(cargo llvm-cov show-env --sh)"),
        LIMIT + re.escape("cargo test --locked"),
        re.escape("cargo llvm-cov report --json | python3 .github/coverage.py"),
    ],
    ("test.yml", "msrv"): [re.escape("cargo build --lib --locked")],
    ("lint.yml", "markdown"): [re.escape("npx --yes markdownlint-cli2@") + r"[0-9.]+"],
    ("lint.yml", "clippy"): [re.escape("cargo clippy --workspace --all-targets --locked")],
    ("lint.yml", "fmt"): [re.escape("cargo fmt --all --check")],
    ("lint.yml", "doc"): [re.escape("cargo doc --no-deps --locked")],
    ("publish.yml", "verify"): [LIMIT + re.escape("cargo test --locked"), re.escape("cargo package --locked -p tailq")],
}

# The only job that may be skipped: publish runs on merged PRs from rc/* branches alone.
SKIPPABLE = {("publish.yml", "verify")}
# A run line that cannot fail.
SOFT = re.compile(r"\|\|\s*(true|:|exit\s+0)\b|\bset\s+\+e\b")


def load(name: str):
    spec = importlib.util.spec_from_file_location(name.removesuffix(".py"), GITHUB / name)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stub(path: pathlib.Path, body: str) -> None:
    path.write_text(f"#!/bin/bash\n{body}\n")
    path.chmod(0o755)


def miri_runs(job: dict) -> dict[str, list[tuple[bool, list[str], list[str]]]]:
    """Each matrix entry's runs of the limited steps: whether limit.sh ran it, MIRIFLAGS, and cargo's arguments."""
    runs = {}
    steps = [step for step in job.get("steps", []) if ".github/limit.sh" in step.get("run", "")]
    with tempfile.TemporaryDirectory() as scratch:
        root = pathlib.Path(scratch)
        (root / ".github").mkdir()
        (root / "bin").mkdir()
        log = root / "runs"
        stub(root / ".github/limit.sh", 'LIMITED=1 exec "$@"')
        stub(root / "bin/cargo", 'printf "%s\\t%s\\t%s\\n" "${LIMITED:-0}" "$MIRIFLAGS" "$*" >> "$RUNS"')
        for entry in job.get("strategy", {}).get("matrix", {}).get("include", []):

            def expand(value: object) -> str:
                return re.sub(r"\$\{\{\s*matrix\.(\w+)\s*\}\}", lambda m: str(entry.get(m.group(1), "")), str(value))

            log.write_text("")
            env = {"PATH": f"{root / 'bin'}:/usr/bin:/bin", "RUNS": str(log)}
            for step in steps:
                env |= {key: expand(value) for key, value in step.get("env", {}).items()}
                # GitHub runs a step with no shell as bash -e.
                subprocess.run(["bash", "-e", "-c", step["run"]], cwd=root, env=env, check=False, timeout=60,
                               capture_output=True)
            runs[str(entry.get("name", entry))] = [
                (limited == "1", flags.split(), args.split())
                for limited, flags, args in (line.split("\t") for line in log.read_text().splitlines())
            ]
    return runs


def check_miri(workflows: dict[str, dict]) -> list[str]:
    runs = miri_runs(workflows["test.yml"]["jobs"].get("miri", {}))
    problems = []
    for what, needs, refuses, args, seeds in MIRI:

        def meets(entry: list[tuple[bool, list[str], list[str]]]) -> bool:
            chosen = {flag for _, flags, _ in entry for flag in flags if flag.startswith("-Zmiri-seed=")}
            return len(entry) >= seeds and (seeds == 1 or len(chosen) >= seeds) and all(
                limited and words == ["miri", "test", "--locked", *args] and needs <= set(flags)
                and not refuses & set(flags)
                for limited, flags, words in entry
            )

        if not any(meets(entry) for entry in runs.values()):
            problems.append(f"test.yml: Miri no longer runs {what}")
    return problems


def check_nightly_doctests(workflows: dict[str, dict]) -> list[str]:
    """Only nightly rustdoc checks a compile_fail block's error code."""
    steps = workflows["test.yml"]["jobs"].get("compile-fail", {}).get("steps", [])
    toolchains = [str((step.get("with") or {}).get("toolchain", "")) for step in steps
                  if step.get("uses") == "./.github/actions/setup-rust"]
    if toolchains and all(toolchain.startswith("nightly") for toolchain in toolchains):
        return []
    return ["test.yml: the compile-fail job no longer runs the doctests on nightly"]


def check_commands(workflows: dict[str, dict]) -> list[str]:
    problems = []
    for (name, job), patterns in COMMANDS.items():
        lines = [line.strip() for step in workflows[name]["jobs"].get(job, {}).get("steps", [])
                 for line in step.get("run", "").splitlines()]
        at = 0
        for pattern in patterns:
            found = next((i for i in range(at, len(lines)) if re.fullmatch(pattern, lines[i])), None)
            if found is None:
                problems.append(f"{name}: job {job} no longer runs {pattern.replace(chr(92), '')}, in order")
                break
            at = found + 1
    return problems


def check_soft_failures(workflows: dict[str, dict]) -> list[str]:
    """A job or step that may fail, or be skipped, passes the PR whatever it found."""
    problems = []
    for name, workflow in workflows.items():
        for job, body in workflow["jobs"].items():
            if "continue-on-error" in body or ("if" in body and (name, job) not in SKIPPABLE):
                problems.append(f"{name}: job {job} may fail or be skipped")
            for step in body.get("steps", []):
                if "continue-on-error" in step or "if" in step or SOFT.search(step.get("run", "")):
                    problems.append(f"{name}: job {job} has a step that may fail or be skipped")
    return problems


def check_triggers(workflows: dict[str, dict]) -> list[str]:
    """test.yml and lint.yml run on every push to every PR.  PyYAML reads the key on as True."""
    problems = []
    for name in ("test.yml", "lint.yml"):
        on = workflows[name].get("on", workflows[name].get(True))
        events = {on: None} if isinstance(on, str) else dict.fromkeys(on) if isinstance(on, list) else on
        if not isinstance(events, dict) or "pull_request" not in events:
            problems.append(f"{name} no longer runs on pull requests")
            continue
        pull = events["pull_request"] or {}
        types = set(pull.get("types", ["opened", "synchronize", "reopened"]))
        filters = pull.keys() & {"branches", "branches-ignore", "paths", "paths-ignore"}
        if not {"opened", "synchronize", "reopened"} <= types or filters:
            problems.append(f"{name} no longer runs on every push to every PR")
    return problems


RULES = [check_miri, check_nightly_doctests, check_commands, check_soft_failures, check_triggers]

# A mutation of a workflow, text or a pattern, and the rule that must flag it: each is what review 8 found every
# check passed.
S390X = "          - name: s390x\n            flags: -Zmiri-strict-provenance -Zmiri-tree-borrows\n"
MUTATIONS = [
    ("test.yml", S390X + "            args: --target s390x-unknown-linux-gnu\n", "", check_miri),
    ("test.yml", "            seeds: 16\n", "", check_miri),
    ("test.yml", ".github/limit.sh cargo miri test --locked $ARGS\n", ".github/limit.sh cargo miri test --locked\n",
     check_miri),
    ("test.yml", 'MIRIFLAGS="$FLAGS${SEEDS:+ -Zmiri-seed=$seed}"', 'MIRIFLAGS="${SEEDS:+-Zmiri-seed=$seed}"',
     check_miri),
    ("test.yml", "-Zmiri-seed=$seed", "-Zmiri-seed=0", check_miri),
    ("test.yml", "-Zmiri-strict-provenance -Zmiri-symbolic-alignment-check", "-Zmiri-strict-provenance", check_miri),
    ("test.yml", "args: --release\n", "args: --release --lib\n", check_miri),
    ("test.yml", "-Zmiri-symbolic-alignment-check\n", "-Zmiri-symbolic-alignment-check -Zmiri-tree-borrows\n",
     check_miri),
    ("test.yml", 'MIRIFLAGS="$FLAGS${SEEDS:+ -Zmiri-seed=$seed}" .github/limit.sh cargo',
     '.github/limit.sh true; MIRIFLAGS="$FLAGS${SEEDS:+ -Zmiri-seed=$seed}" cargo', check_miri),
    # Whatever nightly it pins, the job's own: the miri job's comes first.
    ("test.yml", re.compile(r"(?<=  compile-fail:\n)((?:.*\n)*?          toolchain: )nightly\S*"), r"\g<1>1.99.0",
     check_nightly_doctests),
    ("test.yml", "      - run: .github/limit.sh cargo test --locked --release\n",
     "      - run: .github/limit.sh cargo test --locked\n", check_commands),
    ("test.yml", "cargo llvm-cov report --json | python3 .github/coverage.py\n", "cargo llvm-cov report --json\n",
     check_commands),
    ("test.yml", "      - run: cargo build --lib --locked\n", "", check_commands),
    # The tests run before show-env instruments them.
    ("test.yml",
     re.compile(r"( +source <\(cargo llvm-cov show-env --sh\)\n)((?:.*\n)*? +\.github/limit\.sh cargo test .*\n)"),
     r"\2\1", check_commands),
    ("lint.yml", "      - run: cargo fmt --all --check\n", "      - run: cargo fmt --all\n", check_commands),
    ("lint.yml", "      - run: cargo doc --no-deps --locked\n", "", check_commands),
    ("test.yml", "      - run: .github/limit.sh cargo test --locked\n      # Release",
     "      - run: .github/limit.sh cargo test --locked\n        continue-on-error: true\n      # Release",
     check_soft_failures),
    ("test.yml", "  compile-fail:\n", "  compile-fail:\n    if: false\n", check_soft_failures),
    ("lint.yml", "      - run: cargo fmt --all --check\n", "      - run: cargo fmt --all --check || true\n",
     check_soft_failures),
    ("test.yml", "types: [opened, synchronize, reopened]", "types: [opened, reopened]", check_triggers),
    ("lint.yml", "types: [opened, synchronize, reopened]",
     "types: [opened, synchronize, reopened]\n    paths: [src/**]", check_triggers),
]


def workflows_of(texts: dict[str, str]) -> dict[str, dict]:
    return {name: yaml.safe_load(text) for name, text in texts.items()}


def check_rules() -> list[str]:
    """Each rule flags the mutations it must; the real workflows pass every rule, which main() sees."""
    texts = {path.name: path.read_text() for path in sorted((GITHUB / "workflows").glob("*.yml"))}
    problems = []
    for name, old, new, rule in MUTATIONS:
        pattern = old if isinstance(old, re.Pattern) else re.compile(re.escape(old))
        mutated, count = pattern.subn(new.replace("\\", "\\\\") if pattern is not old else new, texts[name])
        shown = (pattern.pattern if pattern is old else old).strip()
        if count != 1:
            problems.append(f"check_runs.py's mutation of {name} no longer applies: {shown!r}")
        elif not rule(workflows_of(texts | {name: mutated})):
            problems.append(f"check_runs.py's {rule.__name__} passes {name} with {shown!r} -> {new.strip()!r}")
    return problems


# Each checker, and the rules its main() must run: a main() that dropped one passed with it.
CHECKERS = {
    "check_workflows.py": ("check_workflows", "check_job_rules", "check_coverage_script"),
    "check_limits.py": ("check_commands", "check_limit_script", "check_lint_job"),
    "check_limits_test.py": ("check_lines", "check_limit_scripts", "check_lint_jobs"),
    "check_workflows_test.py": ("check_jobs", "check_main", "check_steps"),
    "check_runs.py": ("check_all", "check_rules", "check_mains", "check_step_rules"),
}


def check_mains() -> list[str]:
    problems = []
    for script, rules in CHECKERS.items():
        module = load(script)
        real = {rule: getattr(module, rule) for rule in rules}
        try:
            for planted in (None, *rules):
                for rule in rules:
                    setattr(module, rule, (lambda *_: ["planted"]) if rule == planted else (lambda *_: []))
                with contextlib.redirect_stdout(io.StringIO()):
                    status = module.main()
                if status != (0 if planted is None else 1):
                    problems.append(f"{script}'s main() " + (f"no longer runs {planted}" if planted else "fails"))
        finally:
            for rule, function in real.items():
                setattr(module, rule, function)
    return problems


# A mutation of test.yml, and what check_workflows_test.py's check_steps must then find missing.
STEPS = [
    ("      - run: .github/limit.sh cargo test --locked -p tailq-flip\n", "", "the flip crate's tests"),
    ("cargo test --locked --doc -p tailq -p tailq-flip", "cargo test --locked --doc -p tailq", "the flip doctests"),
    ("cargo test --locked --doc -p tailq -p tailq-flip", "cargo test --locked -p tailq -p tailq-flip",
     "the doctests"),
    ("          cargo package --locked -p tailq\n", "", "the packaging"),
    ('          .github/limit.sh cargo test --locked --manifest-path "target/package/tailq-$crate/Cargo.toml"\n', "",
     "the packaged crate's tests"),
]


def check_step_rules() -> list[str]:
    module = load("check_workflows_test.py")
    text = (GITHUB / "workflows/test.yml").read_text()
    problems = []
    with tempfile.TemporaryDirectory() as scratch:
        root = pathlib.Path(scratch)
        (root / "workflows").mkdir()
        shutil.copy(GITHUB / "workflows/lint.yml", root / "workflows/lint.yml")
        real = module.GITHUB
        try:
            module.GITHUB = root
            (root / "workflows/test.yml").write_text(text)
            if module.check_steps():
                problems.append("check_workflows_test.py's check_steps fails test.yml")
            for old, new, what in STEPS:
                if text.count(old) != 1:
                    problems.append(f"check_runs.py's mutation of test.yml no longer applies: {old.strip()!r}")
                    continue
                (root / "workflows/test.yml").write_text(text.replace(old, new))
                if not module.check_steps():
                    problems.append(f"check_workflows_test.py's check_steps passes test.yml without {what}")
        finally:
            module.GITHUB = real
    return problems


def check_all() -> list[str]:
    workflows = workflows_of({path.name: path.read_text() for path in sorted((GITHUB / "workflows").glob("*.yml"))})
    return [problem for rule in RULES for problem in rule(workflows)]


def main() -> int:
    problems = check_all() + check_rules() + check_mains() + check_step_rules()
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
