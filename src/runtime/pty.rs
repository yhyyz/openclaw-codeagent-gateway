//! PTY-based process I/O handling.

use std::collections::HashMap;

use regex::Regex;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::error::Error;

// Matches ANSI escape sequences (colors, cursor show/hide)
const ANSI_RE: &str = r"\x1b\[[0-9;]*[A-Za-z]|\x1b\[\?25[hl]";

pub async fn run_pty(
    command: &str,
    args: &[String],
    prompt: &str,
    cwd: &str,
    env: &HashMap<String, String>,
) -> Result<(String, Vec<String>), Error> {
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd.arg(prompt);
    cmd.current_dir(cwd);
    cmd.env_clear();
    cmd.env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()));
    cmd.env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    cmd.env("TERM", "dumb");
    cmd.env("LANG", "en_US.UTF-8");
    for (k, v) in env {
        cmd.env(k, v);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        Error::AgentCrashed("failed to capture pty stdout".into())
    })?;

    let ansi_re = Regex::new(ANSI_RE).expect("invalid ANSI regex");
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut output = String::new();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let stripped = ansi_re.replace_all(&line, "");
                output.push_str(&stripped);
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        return Err(Error::AgentCrashed(format!(
            "command '{}' exited with status {}",
            command,
            status.code().unwrap_or(-1)
        )));
    }

    let trimmed = output.trim().to_string();
    Ok((trimmed, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_echo_returns_output() {
        let (output, tools) = run_pty(
            "echo",
            &[],
            "hello world",
            "/tmp",
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(output, "hello world");
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn run_nonexistent_command_fails() {
        let result = run_pty(
            "/nonexistent/binary/99999",
            &[],
            "test",
            "/tmp",
            &HashMap::new(),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_false_returns_error() {
        let result = run_pty(
            "false",
            &[],
            "",
            "/tmp",
            &HashMap::new(),
        )
        .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exited with status"));
    }

    #[tokio::test]
    async fn strips_ansi_codes() {
        // printf outputs ANSI escape then text
        let (output, _) = run_pty(
            "printf",
            &[],
            "\x1b[31mred text\x1b[0m",
            "/tmp",
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert!(!output.contains("\x1b["));
        assert!(output.contains("red text"));
    }

    #[tokio::test]
    async fn env_vars_passed_to_command() {
        let mut env = HashMap::new();
        env.insert("MY_TEST_VAR".into(), "test_value_42".into());
        let (output, _) = run_pty(
            "sh",
            &["-c".into()],
            "echo $MY_TEST_VAR",
            "/tmp",
            &env,
        )
        .await
        .unwrap();
        assert_eq!(output, "test_value_42");
    }
}
