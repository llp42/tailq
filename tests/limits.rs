// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Every test runs through `.github/limit.sh`, the runner that
//! `.cargo/config.toml` names: at most 512 MiB of data memory, under a
//! `timeout` of at most ten minutes.  This fails when a test runs without
//! it, or under a looser one.  It reads Linux's `/proc`, which Miri, which
//! runs under `limit.sh` as a whole, does not offer.

#![cfg(all(target_os = "linux", not(miri)))]

use std::fs;

/// The most data memory a test may use.
const MEMORY: u64 = 512 << 20;
/// The longest a test may run, in seconds.
const SECONDS: u64 = 600;

#[test]
fn tests_run_under_a_memory_limit_and_a_timeout() {
    let limits = fs::read_to_string("/proc/self/limits").unwrap();
    let data = limits
        .lines()
        .find_map(|line| line.strip_prefix("Max data size"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|soft| soft.parse::<u64>().ok());
    assert!(
        data.is_some_and(|data| data <= MEMORY),
        "a test may use {data:?} bytes of data, more than {MEMORY}: run it through .github/limit.sh",
    );
    // The parent's id follows the command name, which may hold spaces, in
    // parentheses.
    let stat = fs::read_to_string("/proc/self/stat").unwrap();
    let parent = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(1))
        .unwrap_or_default();
    let command =
        fs::read_to_string(format!("/proc/{parent}/cmdline")).unwrap();
    let words: Vec<&str> = command.split('\0').collect();
    let seconds = words
        .iter()
        .skip(1)
        .find(|word| !word.starts_with('-'))
        .and_then(|word| word.parse::<u64>().ok());
    assert!(
        words
            .first()
            .is_some_and(|program| program.ends_with("timeout"))
            && seconds.is_some_and(|seconds| seconds <= SECONDS),
        "a test runs under {words:?}, not a timeout of at most {SECONDS} seconds: run it through .github/limit.sh",
    );
}
