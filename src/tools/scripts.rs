use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Structured metadata for an automation script in the scripts repository
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScriptInfo {
    pub name: String,
    pub path: PathBuf,
    pub interpreter: String,
    pub description: String,
    pub usage: String,
    pub size_bytes: u64,
}

/// Dynamic Script-RAG and automation execution engine for client scripts
#[derive(Clone)]
pub struct ScriptsTool {
    dir: Arc<Mutex<PathBuf>>,
    timeout_secs: u64,
}

impl ScriptsTool {
    /// Initialize with auto-detected `scripts/` directory
    pub fn new() -> Self {
        let default_dir = find_or_create_scripts_dir();
        Self {
            dir: Arc::new(Mutex::new(default_dir)),
            timeout_secs: 60,
        }
    }

    pub fn with_dir<P: AsRef<Path>>(dir: P) -> Self {
        let p = dir.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&p);
        Self {
            dir: Arc::new(Mutex::new(p)),
            timeout_secs: 60,
        }
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.dir.lock().unwrap().clone()
    }

    pub fn set_scripts_dir<P: AsRef<Path>>(&self, path: P) {
        let mut dir = self.dir.lock().unwrap();
        *dir = path.as_ref().to_path_buf();
    }

    /// Scan `scripts/` directory and parse metadata from each script
    pub fn scan_catalog(&self) -> Vec<ScriptInfo> {
        let dir = self.scripts_dir();
        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
            return Vec::new();
        }

        let mut catalog = Vec::new();

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => return catalog,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // Ignore hidden files and README files
            if file_name.starts_with('.') || file_name.to_lowercase().starts_with("readme") {
                continue;
            }

            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let size_bytes = metadata.len();

            // Read the first 4KB to parse comments, docstrings, and shebang
            let header_sample = read_header_sample(&path).unwrap_or_default();
            let interpreter = detect_interpreter(&file_name, &header_sample);
            let (description, usage) = parse_docstring_and_usage(&header_sample, &file_name);

            catalog.push(ScriptInfo {
                name: file_name,
                path,
                interpreter,
                description,
                usage,
                size_bytes,
            });
        }

        catalog.sort_by(|a, b| a.name.cmp(&b.name));
        catalog
    }

    /// Formats the Script-RAG prompt snippet to inject into the LLM system prompt
    pub fn format_rag_prompt(&self) -> String {
        let catalog = self.scan_catalog();
        let dir = self.scripts_dir();

        if catalog.is_empty() {
            return format!(
                "AUTOMATION SCRIPT ARSENAL (Script-RAG):\n\
                 Directory: {}\n\
                 Status: Directory is ready for client scripts. No scripts currently found.\n\
                 Note: If the user asks for a repeatable automation, you can create a reusable script in `scripts/` using `script_create`.",
                dir.display()
            );
        }

        let mut output = format!(
            "AUTOMATION SCRIPT ARSENAL (Script-RAG):\n\
             Directory: {}\n\
             The client has provided the following custom automation scripts. ALWAYS check this list FIRST before running raw shell commands:\n",
            dir.display()
        );

        for script in &catalog {
            output.push_str(&format!(
                "- `{}` [{}]: {}\n  Usage: {}\n",
                script.name, script.interpreter, script.description, script.usage
            ));
        }

        output.push_str(
            "\nTo execute any script above, use: Action: script_run: <script_name> [arguments]\n\
             To inspect a script's code, use: Action: script_inspect: <script_name>\n\
             To add a new automation script, use: Action: script_create: {\"name\": \"...\", \"content\": \"...\"}"
        );

        output
    }

    /// List all scripts in human-readable formatted text
    pub fn list_scripts(&self) -> Result<String, String> {
        let catalog = self.scan_catalog();
        let dir = self.scripts_dir();

        if catalog.is_empty() {
            return Ok(format!(
                "📁 Scripts Directory: {}\nNo automation scripts found. Drop `.py`, `.sh`, `.js` scripts into this folder or use `script_create`.",
                dir.display()
            ));
        }

        let mut out = format!(
            "📁 Available Client Automation Scripts ({} found in {}):\n\n",
            catalog.len(),
            dir.display()
        );

        for (i, s) in catalog.iter().enumerate() {
            out.push_str(&format!(
                "{}. 📜 {}\n   • Language/Runner : {}\n   • Purpose         : {}\n   • Usage           : {}\n   • Size            : {} bytes\n\n",
                i + 1,
                s.name,
                s.interpreter,
                s.description,
                s.usage,
                s.size_bytes
            ));
        }

        Ok(out.trim_end().to_string())
    }

    /// Inspect a script's source code and header documentation
    pub fn inspect_script(&self, name: &str) -> Result<String, String> {
        let clean_name = name.trim().trim_matches('"').trim_matches('\'');
        let file_path = self.scripts_dir().join(clean_name);

        if !file_path.exists() {
            return Err(format!(
                "Script '{}' not found in scripts directory: {}",
                clean_name,
                self.scripts_dir().display()
            ));
        }

        let content = fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read script '{}': {}", clean_name, e))?;

        let sample = read_header_sample(&file_path).unwrap_or_default();
        let (desc, usage) = parse_docstring_and_usage(&sample, clean_name);
        let interpreter = detect_interpreter(clean_name, &sample);

        Ok(format!(
            "📜 Script: {}\n• Runner: {}\n• Purpose: {}\n• Usage: {}\n• Path: {}\n\n--- Source Code ---\n{}",
            clean_name,
            interpreter,
            desc,
            usage,
            file_path.display(),
            content
        ))
    }

    /// Execute a script by name with raw arguments
    pub async fn execute_script(&self, name: &str, raw_args: &str) -> Result<String, String> {
        let clean_name = name.trim().trim_matches('"').trim_matches('\'');
        if clean_name.is_empty() {
            return Err("No script name specified".to_string());
        }

        let script_path = self.scripts_dir().join(clean_name);
        if !script_path.exists() {
            return Err(format!(
                "Script '{}' does not exist in {}. Use `script_list` to see available scripts.",
                clean_name,
                self.scripts_dir().display()
            ));
        }

        // Ensure executable permissions on Unix
        #[cfg(unix)]
        {
            if let Ok(metadata) = fs::metadata(&script_path) {
                let mut perms = metadata.permissions();
                let mode = perms.mode();
                if mode & 0o111 == 0 {
                    perms.set_mode(mode | 0o755);
                    let _ = fs::set_permissions(&script_path, perms);
                }
            }
        }

        let sample = read_header_sample(&script_path).unwrap_or_default();
        let interpreter = detect_interpreter(clean_name, &sample);

        // Parse arguments into tokens
        let args_vec = parse_arguments(raw_args);

        // Construct tokio Command
        let mut cmd = match interpreter.as_str() {
            "python3" => {
                let mut c = Command::new("python3");
                c.arg(&script_path);
                c
            }
            "bash" | "sh" => {
                let mut c = Command::new("bash");
                c.arg(&script_path);
                c
            }
            "node" => {
                let mut c = Command::new("node");
                c.arg(&script_path);
                c
            }
            _ => Command::new(&script_path),
        };

        for arg in args_vec {
            cmd.arg(arg);
        }

        cmd.current_dir(self.scripts_dir());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let start_time = std::time::Instant::now();

        let run_future = async {
            let child = cmd
                .spawn()
                .map_err(|e| format!("Failed to spawn script '{}': {}", clean_name, e))?;
            child
                .wait_with_output()
                .await
                .map_err(|e| format!("Script execution failed: {}", e))
        };

        let output = match timeout(Duration::from_secs(self.timeout_secs), run_future).await {
            Ok(res) => res?,
            Err(_) => {
                return Err(format!(
                    "Script '{}' timed out after {} seconds.",
                    clean_name, self.timeout_secs
                ));
            }
        };

        let elapsed = start_time.elapsed().as_secs_f64();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        let status_str = if output.status.success() {
            "🟢 Success"
        } else {
            "🔴 Failed"
        };

        let mut res = format!(
            "Script '{}' finished in {:.2}s with status {} (Code: {})\n",
            clean_name,
            elapsed,
            status_str,
            output.status.code().unwrap_or(-1)
        );

        if !stdout.is_empty() {
            res.push_str(&format!("\n[stdout]:\n{}", truncate_text(&stdout, 3000)));
        }

        if !stderr.is_empty() {
            res.push_str(&format!("\n[stderr]:\n{}", truncate_text(&stderr, 2000)));
        }

        if stdout.is_empty() && stderr.is_empty() {
            res.push_str("\n(No console output produced)");
        }

        if output.status.success() {
            Ok(res)
        } else {
            Err(res)
        }
    }

    /// Create or overwrite an automation script in `scripts/` with `0o755` permissions
    pub fn create_script(&self, name: &str, content: &str) -> Result<String, String> {
        let clean_name = name.trim().trim_matches('"').trim_matches('\'');
        if clean_name.is_empty() {
            return Err("Script name cannot be empty".to_string());
        }

        if clean_name.contains('/') || clean_name.contains('\\') || clean_name.starts_with('.') {
            return Err(format!("Invalid script name: '{}'", clean_name));
        }

        let dir = self.scripts_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create scripts directory: {}", e))?;
        }

        let file_path = dir.join(clean_name);

        fs::write(&file_path, content)
            .map_err(|e| format!("Failed to write script '{}': {}", clean_name, e))?;

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&file_path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&file_path, perms);
        }

        let sample = read_header_sample(&file_path).unwrap_or_default();
        let (desc, usage) = parse_docstring_and_usage(&sample, clean_name);
        let interpreter = detect_interpreter(clean_name, &sample);

        Ok(format!(
            "✅ Successfully created automation script '{}' in {}\n• Runner: {}\n• Purpose: {}\n• Usage: {}",
            clean_name,
            dir.display(),
            interpreter,
            desc,
            usage
        ))
    }
}

/// Helper to read first 4KB of a file
fn read_header_sample(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = fs::File::open(path).ok()?;
    let mut buffer = [0u8; 4096];
    let n = file.read(&mut buffer).ok()?;
    String::from_utf8(buffer[..n].to_vec()).ok()
}

/// Helper to detect interpreter from extension or shebang
fn detect_interpreter(file_name: &str, header: &str) -> String {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".py") {
        return "python3".to_string();
    }
    if lower.ends_with(".sh") || lower.ends_with(".bash") {
        return "bash".to_string();
    }
    if lower.ends_with(".js") || lower.ends_with(".mjs") {
        return "node".to_string();
    }
    if lower.ends_with(".rb") {
        return "ruby".to_string();
    }

    if let Some(first_line) = header.lines().next() {
        if first_line.starts_with("#!") {
            let line = first_line.to_lowercase();
            if line.contains("python") {
                return "python3".to_string();
            }
            if line.contains("bash") {
                return "bash".to_string();
            }
            if line.contains("sh") {
                return "sh".to_string();
            }
            if line.contains("node") {
                return "node".to_string();
            }
        }
    }

    "executable".to_string()
}

/// Helper to parse docstring and usage from script header
fn parse_docstring_and_usage(header: &str, file_name: &str) -> (String, String) {
    let mut description = String::new();
    let mut usage = String::new();

    let mut in_py_docstring = false;

    for line in header.lines() {
        let trimmed = line.trim();

        // Python docstrings """
        if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
            if in_py_docstring {
                in_py_docstring = false;
                continue;
            } else {
                in_py_docstring = true;
                let rest = trimmed.trim_matches('"').trim_matches('\'').trim();
                if !rest.is_empty() && description.is_empty() {
                    description = rest.to_string();
                }
                continue;
            }
        }

        if in_py_docstring {
            // Check for explicit tags inside docstring
            if let Some(d) = strip_prefix_case_insensitive(trimmed, "description:") {
                description = d.trim().to_string();
            } else if let Some(u) = strip_prefix_case_insensitive(trimmed, "usage:") {
                usage = u.trim().to_string();
            } else if description.is_empty() && !trimmed.is_empty() {
                description = trimmed.to_string();
            }
            continue;
        }

        // Explicit Description or Summary tags
        if let Some(d) = strip_prefix_case_insensitive(trimmed, "# description:")
            .or_else(|| strip_prefix_case_insensitive(trimmed, "// description:"))
            .or_else(|| strip_prefix_case_insensitive(trimmed, "# summary:"))
            .or_else(|| strip_prefix_case_insensitive(trimmed, "// summary:"))
        {
            description = d.trim().to_string();
        }

        // Explicit Usage tags
        if let Some(u) = strip_prefix_case_insensitive(trimmed, "# usage:")
            .or_else(|| strip_prefix_case_insensitive(trimmed, "// usage:"))
        {
            usage = u.trim().to_string();
        }

        // Fallback: extract first substantial comment line as description
        if description.is_empty()
            && (trimmed.starts_with('#') || trimmed.starts_with("//"))
            && !trimmed.starts_with("#!")
        {
            let comment = trimmed.trim_start_matches('#').trim_start_matches('/').trim();
            if !comment.is_empty()
                && !comment.to_lowercase().starts_with("encoding")
                && !comment.to_lowercase().starts_with("license")
            {
                description = comment.to_string();
            }
        }
    }

    if description.is_empty() {
        description = format!("Automation script '{}'", file_name);
    }

    if usage.is_empty() {
        usage = format!("{} [arguments]", file_name);
    }

    (description, usage)
}

fn strip_prefix_case_insensitive<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn parse_arguments(raw: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for ch in raw.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            q if in_quotes && q == quote_char => {
                in_quotes = false;
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

fn find_or_create_scripts_dir() -> PathBuf {
    // 1. Current working directory / scripts
    let cwd_scripts = PathBuf::from("scripts");
    if cwd_scripts.exists() {
        return cwd_scripts;
    }

    // 2. Look for project directory in ~/Desktop/ClawMind/scripts
    if let Ok(home) = std::env::var("HOME") {
        let clawmind_scripts = PathBuf::from(home).join("Desktop/ClawMind/scripts");
        if clawmind_scripts.exists() {
            return clawmind_scripts;
        }
    }

    // 3. Fall back to creating ./scripts
    let _ = fs::create_dir_all(&cwd_scripts);
    cwd_scripts
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{}\n[... truncated {} characters]", truncated, chars.len() - max_chars)
    }
}
