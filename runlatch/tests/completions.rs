//! Integration tests for shell completion support.
//!
//! Two things are tested:
//! 1. The generated scripts are syntactically valid in their respective shells.
//! 2. `runlatch complete entries/sources` outputs lines in the expected format.

use std::io::Write;
use std::process::{Command, Stdio};

fn runlatch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_runlatch"))
}

// ── Script syntax checks ──────────────────────────────────────────────────────

/// Run a shell with `-n` (parse/syntax-check only) against `script`.
/// Returns Ok(()) if the shell is not installed (skip rather than fail).
fn syntax_check(shell: &str, script: &str) -> Result<(), String> {
    let mut child = match Command::new(shell)
        .arg("-n")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        // Shell not on PATH: treat as skipped.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("spawn {shell}: {e}")),
    };

    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .map_err(|e| format!("write stdin: {e}"))?;

    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{shell} -n exited {:?}:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

#[test]
fn bash_script_is_syntactically_valid() {
    let out = runlatch()
        .args(["completions", "bash"])
        .output()
        .expect("runlatch completions bash");
    assert!(out.status.success());
    syntax_check("bash", &String::from_utf8(out.stdout).unwrap())
        .expect("bash script has syntax errors");
}

#[test]
fn fish_script_is_syntactically_valid() {
    let out = runlatch()
        .args(["completions", "fish"])
        .output()
        .expect("runlatch completions fish");
    assert!(out.status.success());
    // fish -n reads from stdin in interactive mode but not directly; use a temp file.
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&out.stdout).unwrap();
    let status = Command::new("fish")
        .args(["-n", tmp.path().to_str().unwrap()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) => assert!(s.success(), "fish -n reported syntax errors"),
        // fish not installed: skip.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => panic!("spawn fish: {e}"),
    }
}

#[test]
fn zsh_script_is_syntactically_valid() {
    let out = runlatch()
        .args(["completions", "zsh"])
        .output()
        .expect("runlatch completions zsh");
    assert!(out.status.success());
    syntax_check("zsh", &String::from_utf8(out.stdout).unwrap())
        .expect("zsh script has syntax errors");
}

// ── Output format of `complete entries` / `complete sources` ─────────────────

#[test]
fn complete_entries_format() {
    let out = runlatch()
        .args(["complete", "entries"])
        .output()
        .expect("runlatch complete entries");

    assert!(out.status.success(), "exited with {:?}", out.status.code());
    // stderr must be silent — completion scripts can't tolerate noise.
    assert!(
        out.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in stdout.lines() {
        // Each line must be `source:id` — exactly one colon, no surrounding
        // whitespace, no ANSI escapes.
        assert!(
            !line.contains('\x1b'),
            "ANSI escape in completion output: {line:?}"
        );
        let (source, id) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("line missing ':': {line:?}"));
        assert!(!source.is_empty(), "empty source in: {line:?}");
        assert!(!id.is_empty(), "empty id in: {line:?}");
    }
}

#[test]
fn complete_sources_format() {
    let out = runlatch()
        .args(["complete", "sources"])
        .output()
        .expect("runlatch complete sources");

    assert!(out.status.success());
    assert!(
        out.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    for line in stdout.lines() {
        assert!(
            !line.contains('\x1b'),
            "ANSI escape in completion output: {line:?}"
        );
        // Each line is a bare provider id: non-empty, no spaces.
        assert!(!line.is_empty(), "empty line in sources output");
        assert!(
            !line.contains(char::is_whitespace),
            "whitespace in source id: {line:?}"
        );
    }
}
