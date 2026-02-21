use crate::config::Config;
use std::process::Command;

use super::platform::linux_service_file;
use super::utils::{run_capture, run_checked, xml_escape};

#[test]
fn xml_escape_escapes_reserved_chars() {
    let escaped = xml_escape("<&>\"' and text");
    assert_eq!(escaped, "&lt;&amp;&gt;&quot;&apos; and text");
}

#[test]
fn run_capture_reads_stdout() {
    let out = run_capture(Command::new("sh").args(["-lc", "echo hello"]))
        .expect("stdout capture should succeed");
    assert_eq!(out.trim(), "hello");
}

#[test]
fn run_capture_falls_back_to_stderr() {
    let out = run_capture(Command::new("sh").args(["-lc", "echo warn 1>&2"]))
        .expect("stderr capture should succeed");
    assert_eq!(out.trim(), "warn");
}

#[test]
fn run_checked_errors_on_non_zero_status() {
    let err = run_checked(Command::new("sh").args(["-lc", "exit 17"]))
        .expect_err("non-zero exit should error");
    assert!(err.to_string().contains("Command failed"));
}

#[test]
fn linux_service_file_has_expected_suffix() {
    let file = linux_service_file(&Config::default()).unwrap();
    let path = file.to_string_lossy();
    assert!(path.ends_with(".config/systemd/user/asteroniris.service"));
}
