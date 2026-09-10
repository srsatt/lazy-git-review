use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::error::{AppError, Result};

pub fn run(repo: &Path, args: &[&OsStr]) -> Result<Output> {
    let output = command(repo, args).output()?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(AppError::Git {
            command: render_args(args),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

pub fn run_optional(repo: &Path, args: &[&OsStr]) -> Result<Option<Vec<u8>>> {
    let output = command(repo, args).output()?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else if output.status.code() == Some(128) || output.status.code() == Some(1) {
        Ok(None)
    } else {
        Err(AppError::Git {
            command: render_args(args),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

pub fn command(repo: &Path, args: &[&OsStr]) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(repo)
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .args(args);
    command
}

pub fn text(repo: &Path, args: &[&OsStr]) -> Result<String> {
    Ok(String::from_utf8_lossy(&run(repo, args)?.stdout)
        .trim()
        .to_owned())
}

pub fn os(value: &str) -> &OsStr {
    OsStr::new(value)
}

pub fn owned(value: impl Into<OsString>) -> OsString {
    value.into()
}

fn render_args(args: &[&OsStr]) -> String {
    let mut rendered = String::from("git");
    for arg in args {
        rendered.push(' ');
        rendered.push_str(&arg.to_string_lossy());
    }
    rendered
}
