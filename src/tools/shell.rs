use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Shell tool executing terminal commands with working directory tracking and timeout protection.
#[derive(Clone)]
pub struct ShellTool {
    cwd: Arc<Mutex<PathBuf>>,
    timeout_secs: u64,
}

impl ShellTool {
    pub fn new() -> Self {
        let initial_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            cwd: Arc::new(Mutex::new(initial_dir)),
            timeout_secs: 30,
        }
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        let mut s = Self::new();
        s.timeout_secs = timeout_secs;
        s
    }

    pub fn current_dir(&self) -> PathBuf {
        self.cwd.lock().unwrap().clone()
    }

    pub fn set_current_dir<P: AsRef<Path>>(&self, path: P) {
        let mut cwd = self.cwd.lock().unwrap();
        *cwd = path.as_ref().to_path_buf();
    }

    /// Execute a shell command and capture its output
    pub async fn execute(&self, command_line: &str) -> Result<String, String> {
        let trimmed = command_line.trim();
        if trimmed.is_empty() {
            return Err("Empty command provided".to_string());
        }

        // Detect if the command is a directory change
        if trimmed.starts_with("cd ") {
            let target = trimmed[3..].trim();
            let current = self.current_dir();
            let new_path = if target == "~" || target.starts_with("~/") {
                if let Some(home) = dirs_home() {
                    if target == "~" {
                        home
                    } else {
                        home.join(&target[2..])
                    }
                } else {
                    current.join(target)
                }
            } else {
                current.join(target)
            };

            match std::fs::canonicalize(&new_path) {
                Ok(canon) => {
                    if canon.is_dir() {
                        self.set_current_dir(&canon);
                        return Ok(format!("Changed directory to: {}", canon.display()));
                    } else {
                        return Err(format!("Not a directory: {}", new_path.display()));
                    }
                }
                Err(e) => return Err(format!("Cannot access directory {}: {}", new_path.display(), e)),
            }
        }

        let working_dir = self.current_dir();

        // Platform-specific shell
        #[cfg(target_os = "macos")]
        let (shell_bin, shell_arg) = ("/bin/zsh", "-c");
        #[cfg(not(target_os = "macos"))]
        let (shell_bin, shell_arg) = ("/bin/bash", "-c");

        // Check if command contains sudo and whether passwordless sudo is available
        if (trimmed.starts_with("sudo ") || trimmed.contains(" sudo ")) && !can_run_passwordless_sudo() {
            return Err("Command requires sudo/root password which cannot be entered non-interactively. Please run this command manually in your terminal or configure passwordless sudo.".to_string());
        }

        let mut cmd = Command::new(shell_bin);
        cmd.arg(shell_arg)
            .arg(trimmed)
            .current_dir(&working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let execution = async {
            let child = cmd.spawn().map_err(|e| format!("Failed to spawn shell: {e}"))?;
            let output = child
                .wait_with_output()
                .await
                .map_err(|e| format!("Command execution failed: {e}"))?;
            Ok::<_, String>(output)
        };

        match timeout(Duration::from_secs(self.timeout_secs), execution).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                let mut result = String::new();
                if !stdout.trim().is_empty() {
                    result.push_str(&truncate_output(&stdout, 2000));
                }
                if !stderr.trim().is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(&format!("[stderr]: {}", truncate_output(&stderr, 1000)));
                }

                if result.is_empty() {
                    if output.status.success() {
                        result = "(command completed with no output)".to_string();
                    } else {
                        result = format!("(command exited with code {:?})", output.status.code());
                    }
                }

                if !output.status.success() {
                    Err(format!("Exit code {:?}\n{}", output.status.code(), result))
                } else {
                    Ok(result)
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!(
                "Command timed out after {} seconds: {}",
                self.timeout_secs, trimmed
            )),
        }
    }
}

fn truncate_output(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{}\n[... truncated {} characters]", truncated, chars.len() - max_chars)
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn can_run_passwordless_sudo() -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        std::process::Command::new("sudo")
            .arg("-n")
            .arg("true")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        false
    }
}
