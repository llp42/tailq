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
use std::iter;
use std::os::unix::fs::PermissionsExt as _;
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
    // `timeout` itself, or a script of that name its interpreter runs, as
    // Fedora's coreutils-single installs it, then its options exactly as
    // limit.sh writes them: any other spelling could set a signal that ends
    // nothing, or a kill delay `timeout` reads as the duration.
    let seconds = words.windows(3).take(3).find_map(|window| match window {
        [program, "--kill-after=10", seconds]
            if Path::new(program).file_name() == Some("timeout".as_ref()) =>
        {
            seconds.parse::<u64>().ok()
        }
        _ => None,
    });
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

/// The path of `limit.sh`.
const LIMIT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.github/limit.sh");

/// Runs the check under `wrapper`, a `sh -c` script that runs `"$0" "$@"`,
/// with `$LIMIT` the path of `limit.sh`, and returns whether it passed.
fn passes_under(wrapper: &str) -> bool {
    passes_under_limit(wrapper, Path::new(LIMIT))
}

/// As [`passes_under`], with `limit` as `$LIMIT`.
fn passes_under_limit(wrapper: &str, limit: &Path) -> bool {
    let mut command = Command::new("sh");
    let _: &mut Command = command.arg("-c").arg(wrapper).env("LIMIT", limit);
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
    for seconds in ["60", "600"] {
        let wrapper =
            format!("LIMIT_SECONDS={seconds} exec \"$LIMIT\" \"$0\" \"$@\"");
        assert!(passes_under(&wrapper), "the check refuses {wrapper}");
    }
    for seconds in ["0", "601", "299.5"] {
        let wrapper =
            format!("LIMIT_SECONDS={seconds} exec \"$LIMIT\" \"$0\" \"$@\"");
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
/// run under Miri: each must be a whole number from 1 to 600 seconds.
#[test]
fn every_workflow_keeps_the_timeout_within_ten_minutes() {
    let workflows =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    for entry in fs::read_dir(workflows).unwrap() {
        let path = entry.unwrap().path();
        for line in fs::read_to_string(&path).unwrap().lines() {
            if let Some((_, value)) = line.split_once("LIMIT_SECONDS") {
                let value = value
                    .trim_start_matches([':', '=', ' ', '"'])
                    .split(|c: char| c.is_whitespace() || c == '"')
                    .next()
                    .unwrap_or_default();
                assert!(
                    value
                        .parse::<u64>()
                        .is_ok_and(|value| (1..=SECONDS).contains(&value)),
                    "{} runs tests under {}, more than {SECONDS} seconds",
                    path.display(),
                    line.trim(),
                );
            }
        }
    }
}

/// Runs the check under `limit.sh` reached through a path with a space, a
/// copy of it in a directory named `limits copy`: the wrappers name it
/// quoted, as `"$LIMIT"`, so the check passes as it does in any checkout.
#[test]
fn the_check_takes_limit_sh_from_a_path_with_a_space() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("limits copy {}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let limit = dir.join("limit.sh");
    let _: u64 = fs::copy(LIMIT, &limit).unwrap();
    let passed = passes_under_limit(
        "LIMIT_SECONDS=60 exec \"$LIMIT\" \"$0\" \"$@\"",
        &limit,
    );
    fs::remove_dir_all(&dir).unwrap();
    assert!(passed, "the check refuses limit.sh at {}", limit.display());
}

/// A `timeout` that is a script, run by its interpreter, as Fedora's
/// `coreutils-single` installs `/usr/bin/timeout`, a script that reads
/// `#!/usr/bin/coreutils --coreutils-prog-shebang=timeout`.  It runs its
/// command and ends it after its duration, as `timeout` does.
const SCRIPT_TIMEOUT: &str = "#!/usr/bin/python3 -I
import subprocess, sys
kill_after = float(sys.argv[1].removeprefix('--kill-after='))
child = subprocess.Popen(sys.argv[3:])
try:
    sys.exit(child.wait(float(sys.argv[2])))
except subprocess.TimeoutExpired:
    child.terminate()
    try:
        child.wait(kill_after)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
    sys.exit(124)
";

/// Puts a `timeout` that is a script first on the `PATH`, and runs through
/// `limit.sh` a `sleep` of ten seconds, which it ends after one, then the
/// check, under a timeout of a minute: the check passes, as it does under
/// `limit.sh` with any `timeout` the `PATH` finds, though the parent's
/// command line then starts with the script's interpreter.
#[test]
fn the_check_takes_limit_sh_with_a_timeout_that_is_a_script() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("script-timeout-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let timeout = dir.join("timeout");
    fs::write(&timeout, SCRIPT_TIMEOUT).unwrap();
    fs::set_permissions(&timeout, fs::Permissions::from_mode(0o755)).unwrap();
    let search = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(
        iter::once(dir.clone()).chain(env::split_paths(&search)),
    )
    .unwrap();
    let ended = Command::new(LIMIT)
        .args(["sleep", "10"])
        .env("PATH", &path)
        .env("LIMIT_SECONDS", "1")
        .status()
        .unwrap();
    let mut command = Command::new(LIMIT);
    let _: &mut Command = command.env("PATH", &path).env("LIMIT_SECONDS", "60");
    let output = check(command);
    fs::remove_dir_all(&dir).unwrap();
    assert_eq!(ended.code(), Some(124), "the script ends no command");
    assert!(
        output.status.success(),
        "the check refuses limit.sh with a timeout that is a script: {}",
        String::from_utf8_lossy(&output.stdout),
    );
}
