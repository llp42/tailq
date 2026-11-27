// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Every test runs through `.github/limit.sh`, the runner that
//! `.cargo/config.toml` names: at most 512 MiB of data memory, under a
//! `timeout` of at most ten minutes.  This fails when a test runs without
//! it, or under a looser one: its parent must be `timeout` as `limit.sh`
//! runs it, `timeout --kill-after=10 SECONDS`, with SECONDS a whole number
//! from 1 to 600.  It reads Linux's `/proc`, which Miri, which runs under
//! `limit.sh` as a whole, does not offer.

#![cfg(all(target_os = "linux", not(miri)))]

use std::env;
use std::fs;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Output};

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
    // Any other spelling could set a signal that ends nothing, or a kill
    // delay `timeout` reads as the duration.
    let seconds = match words.as_slice() {
        [program, "--kill-after=10", seconds, ..]
            if Path::new(program).file_name() == Some("timeout".as_ref()) =>
        {
            seconds.parse::<u64>().ok()
        }
        _ => None,
    };
    assert!(
        seconds.is_some_and(|seconds| (1..=SECONDS).contains(&seconds)),
        "a test runs under {words:?}, not a timeout of at most {SECONDS} seconds: run it through .github/limit.sh",
    );
}

/// The check above, by its name in this binary.
const CHECK: &str = "tests_run_under_a_memory_limit_and_a_timeout";

/// Runs the check above alone in a child of this binary, under `command`.
fn check(mut command: Command) -> Output {
    let output = command
        .arg(env::current_exe().unwrap())
        .args(["--exact", CHECK, "--test-threads=1"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running 1 test"), "{command:?}: {stdout}");
    output
}

/// Runs the check under `wrapper`, a `sh -c` script that runs `"$0" "$@"`,
/// and returns whether it passed.
fn passes_under(wrapper: &str) -> bool {
    let mut command = Command::new("sh");
    let _: &mut Command = command.arg("-c").arg(wrapper);
    check(command).status.success()
}

/// Runs the check in a child under `limit.sh` with a timeout of a minute
/// and of ten, which it takes, and under wrappers that bound a
/// test less, which it refuses: `limit.sh` with a timeout of zero, which
/// turns it off, of 601 seconds, and of 299.5, which is no whole number;
/// and, with the memory limit, `timeout` with no kill delay, with a kill
/// delay of ten that `-k`, `-vk` or `--kill` puts before a duration of
/// 6000 seconds, and with a signal, `CONT` or the null one, that ends
/// nothing.
#[test]
fn the_check_takes_limit_sh_and_nothing_looser() {
    let limit = concat!(env!("CARGO_MANIFEST_DIR"), "/.github/limit.sh");
    for seconds in ["60", "600"] {
        let wrapper =
            format!("LIMIT_SECONDS={seconds} exec {limit} \"$0\" \"$@\"");
        assert!(passes_under(&wrapper), "the check refuses {wrapper}");
    }
    for seconds in ["0", "601", "299.5"] {
        let wrapper =
            format!("LIMIT_SECONDS={seconds} exec {limit} \"$0\" \"$@\"");
        assert!(!passes_under(&wrapper), "the check passes under {wrapper}");
    }
    for timeout in [
        "600",
        "-k 10 6000",
        "-vk 10 6000",
        "--kill 10 6000",
        "-s CONT --kill-after=10 600",
        "--signal=0 --kill-after=10 600",
    ] {
        let wrapper =
            format!("ulimit -d 524288 && exec timeout {timeout} \"$0\" \"$@\"");
        assert!(!passes_under(&wrapper), "the check passes under {wrapper}");
    }
}

/// Runs the check as the child of a shell started under the name
/// `notimeout` on a script file named `600`, which sets the memory limit
/// and runs the check with no timeout: the parent's command line reads
/// `notimeout 600`.  The check fails, since no `timeout` runs it.
#[test]
fn the_check_fails_under_a_parent_that_only_bears_the_name() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("limits-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("600"), "ulimit -d 524288 && \"$@\"\n").unwrap();
    let mut command = Command::new("sh");
    let _: &mut Command =
        command.arg0("notimeout").arg("600").current_dir(&dir);
    let output = check(command);
    fs::remove_dir_all(&dir).unwrap();
    assert!(
        !output.status.success(),
        "the check passes under a shell named notimeout, with no timeout",
    );
}

/// Reads every workflow for the `LIMIT_SECONDS` it gives `limit.sh`, which
/// the Miri runs would set out of this check's sight, since it does not
/// run under Miri: each must be a whole number of at most 600 seconds.
#[test]
fn every_workflow_keeps_the_timeout_within_ten_minutes() {
    let workflows =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    for entry in fs::read_dir(workflows).unwrap() {
        let path = entry.unwrap().path();
        for line in fs::read_to_string(&path).unwrap().lines() {
            if let Some((_, value)) = line.split_once("LIMIT_SECONDS") {
                let value = value.trim_start_matches([':', '=', ' ', '"']);
                let digits: String =
                    value.chars().take_while(char::is_ascii_digit).collect();
                assert!(
                    digits.parse::<u64>().is_ok_and(|value| value <= SECONDS),
                    "{} runs tests under {}, more than {SECONDS} seconds",
                    path.display(),
                    line.trim(),
                );
            }
        }
    }
}
