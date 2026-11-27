// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Every test runs through `.github/limit.sh`, the runner that
//! `.cargo/config.toml` names: at most 512 MiB of data memory, under a
//! `timeout` of at most ten minutes.  This fails when a test runs without
//! it, or under a looser one.  It reads Linux's `/proc`, which Miri, which
//! runs under `limit.sh` as a whole, does not offer.

#![cfg(all(target_os = "linux", not(miri)))]

use std::env;
use std::fs;
use std::process::Command;

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
    // The duration is `timeout`'s first operand: `-k` and `-s` take the
    // word after them, unless it is joined to them, and `--` ends the
    // options.
    let mut rest = words.iter().skip(1);
    let mut duration = None;
    while let Some(&word) = rest.next() {
        match word {
            "-k" | "--kill-after" | "-s" | "--signal" => {
                let _: Option<&&str> = rest.next();
            }
            "--" => {
                duration = rest.next().copied();
                break;
            }
            _ if word.starts_with('-') => {}
            _ => {
                duration = Some(word);
                break;
            }
        }
    }
    // A duration of zero turns the timeout off.
    let seconds = duration.and_then(|word| word.parse::<u64>().ok());
    assert!(
        words
            .first()
            .is_some_and(|program| program.ends_with("timeout"))
            && seconds.is_some_and(|seconds| (1..=SECONDS).contains(&seconds)),
        "a test runs under {words:?}, not a timeout of at most {SECONDS} seconds: run it through .github/limit.sh",
    );
}

/// Runs the check above alone in a child of this binary, under two
/// wrappers that set no timeout of at most ten minutes: `limit.sh` with
/// `LIMIT_SECONDS=0`, which runs `timeout` with a duration of zero and so
/// with no timeout, and `timeout -k 10 6000`, a timeout of 6000 seconds
/// with a kill delay of ten.  The check fails in both children.
#[test]
fn the_check_fails_without_a_timeout_of_at_most_ten_minutes() {
    const CHECK: &str = "tests_run_under_a_memory_limit_and_a_timeout";
    let exe = env::current_exe().unwrap();
    let mut off =
        Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.github/limit.sh"));
    let _: &mut Command = off.env("LIMIT_SECONDS", "0").arg(&exe).args([
        "--exact",
        CHECK,
        "--test-threads=1",
    ]);
    let mut looser = Command::new("sh");
    let _: &mut Command = looser
        .arg("-c")
        .arg(
            "ulimit -d 524288 && exec timeout -k 10 6000 \"$0\" --exact \"$1\" \
             --test-threads=1",
        )
        .arg(&exe)
        .arg(CHECK);
    for (name, mut run) in
        [("LIMIT_SECONDS=0", off), ("timeout -k 10 6000", looser)]
    {
        let output = run.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("running 1 test"), "{name}: {stdout}");
        assert!(!output.status.success(), "the check passes under {name}");
    }
}
