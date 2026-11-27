#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Fails unless every code region of src/ ran in the tests.

Reads the JSON that `cargo llvm-cov report --json` prints on stdin. A generic function has one
instantiation per type it is used with, and a region counts as run when any instantiation ran it:
llvm-cov's own summary counts each instantiation apart, so a branch that one adapter type never
takes would show as missed although the code ran.
"""

import json
import pathlib
import sys

CODE_REGION = 0
# The crate's own src/, wherever the checkout is: a path that merely contains "/src/", such as
# ~/src/tailq/tests/model.rs, is not in it.
SRC = pathlib.Path(__file__).resolve().parent.parent / "src"


def main() -> int:
    export = json.load(sys.stdin)["data"][0]
    runs: dict[tuple[str, int, int, int, int], int] = {}
    for function in export["functions"]:
        for start, start_col, end, end_col, count, file_id, _, kind in function["regions"]:
            name = function["filenames"][file_id]
            if kind != CODE_REGION or not pathlib.Path(name).resolve().is_relative_to(SRC):
                continue
            key = (name, start, start_col, end, end_col)
            runs[key] = max(runs.get(key, 0), count)
    missed = sorted(key for key, count in runs.items() if count == 0)
    for name, start, start_col, end, end_col in missed:
        print(f"{name}:{start}:{start_col}-{end}:{end_col}: never ran")
    files = len({key[0] for key in runs})
    print(f"{len(runs) - len(missed)} of {len(runs)} code regions ran, in {files} files")
    return 1 if missed or not runs else 0


if __name__ == "__main__":
    sys.exit(main())
