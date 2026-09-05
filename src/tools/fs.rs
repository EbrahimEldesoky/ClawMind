use std::fs;
use std::path::{Path, PathBuf};

/// Filesystem tool for managing files and directories.
#[derive(Clone, Default)]
pub struct FsTool;

impl FsTool {
    pub fn new() -> Self {
        Self
    }

    /// Resolve a path relative to the base directory
    pub fn resolve_path<P: AsRef<Path>>(base: &Path, path: P) -> PathBuf {
        let p = path.as_ref();
        if p.is_absolute() {
            p.to_path_buf()
        } else if let Ok(stripped) = p.strip_prefix("~") {
            if let Ok(home) = std::env::var("HOME") {
                PathBuf::from(home).join(stripped)
            } else {
                base.join(p)
            }
        } else {
            base.join(p)
        }
    }

    /// Write content to a file, creating parent directories if necessary
    pub fn write_file(&self, base: &Path, path_str: &str, content: &str) -> Result<String, String> {
        let p = Path::new(path_str);
        if !p.is_absolute() && !path_str.starts_with('~') {
            if let Ok(cargo_toml) = fs::read_to_string(base.join("Cargo.toml")) {
                if cargo_toml.contains("name = \"clawmind\"")
                    && (path_str.starts_with("src/")
                        || path_str.starts_with("./src/")
                        || path_str == "src"
                        || path_str == "Cargo.toml")
                {
                    return Err(format!(
                        "Refusing to overwrite ClawMind's own source code '{}' with relative path. If you are creating or editing another project, specify the full absolute path (e.g. ~/Desktop/<project>/{}).",
                        path_str, path_str
                    ));
                }
            }
        }

        let path = Self::resolve_path(base, path_str);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directories for {}: {e}", path.display()))?;
            }
        }

        fs::write(&path, content)
            .map_err(|e| format!("Failed to write to {}: {e}", path.display()))?;

        Ok(format!(
            "Successfully wrote {} bytes to {}",
            content.len(),
            path.display()
        ))
    }

    /// Read file contents with optional line limit
    pub fn read_file(&self, base: &Path, path_str: &str, max_lines: Option<usize>) -> Result<String, String> {
        let path = Self::resolve_path(base, path_str);
        if !path.exists() {
            return Err(format!("File does not exist: {}", path.display()));
        }

        if path.is_dir() {
            return Err(format!("Path is a directory, not a file: {}", path.display()));
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

        let limit = max_lines.unwrap_or(200);
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() <= limit {
            Ok(content)
        } else {
            let truncated: String = lines[..limit].join("\n");
            Ok(format!(
                "{}\n\n[... File has {} lines, showing first {}]",
                truncated,
                lines.len(),
                limit
            ))
        }
    }

    /// Delete a file or directory
    pub fn delete_path(&self, base: &Path, path_str: &str) -> Result<String, String> {
        let path = Self::resolve_path(base, path_str);
        if !path.exists() {
            return Err(format!("Target does not exist: {}", path.display()));
        }

        if path.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("Failed to remove directory {}: {e}", path.display()))?;
            Ok(format!("Successfully deleted directory: {}", path.display()))
        } else {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove file {}: {e}", path.display()))?;
            Ok(format!("Successfully deleted file: {}", path.display()))
        }
    }

    /// List directory contents
    pub fn list_dir(&self, base: &Path, path_str: &str) -> Result<String, String> {
        let path = if path_str.trim().is_empty() || path_str == "." {
            base.to_path_buf()
        } else {
            Self::resolve_path(base, path_str)
        };

        if !path.exists() {
            return Err(format!("Directory does not exist: {}", path.display()));
        }

        if !path.is_dir() {
            return Err(format!("Path is not a directory: {}", path.display()));
        }

        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&path)
            .map_err(|e| format!("Failed to read directory {}: {e}", path.display()))?;

        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type().ok();
            let is_dir = file_type.map(|t| t.is_dir()).unwrap_or(false);
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

            if is_dir {
                entries.push(format!("  [DIR]  {}/", name));
            } else {
                entries.push(format!("  [FILE] {:<30} ({} bytes)", name, size));
            }
        }

        entries.sort();

        let header = format!("Contents of {}:\n", path.display());
        if entries.is_empty() {
            Ok(format!("{}(empty directory)", header))
        } else {
            Ok(format!("{}{}", header, entries.join("\n")))
        }
    }

    /// Analyze a file: type, size, line count, structure
    pub fn analyze_file(&self, base: &Path, path_str: &str) -> Result<String, String> {
        let path = Self::resolve_path(base, path_str);
        if !path.exists() {
            return Err(format!("Target does not exist: {}", path.display()));
        }

        let metadata = fs::metadata(&path)
            .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?;

        if metadata.is_dir() {
            let count = fs::read_dir(&path).map(|r| r.count()).unwrap_or(0);
            return Ok(format!(
                "Directory Analysis: {}\n• Type: Directory\n• Direct Items: {}\n• Permissions: readonly={}",
                path.display(),
                count,
                metadata.permissions().readonly()
            ));
        }

        let size = metadata.len();
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("none");

        let mut analysis = format!(
            "File Analysis: {}\n• Extension: {}\n• Size: {} bytes ({:.2} KB)\n",
            path.display(),
            extension,
            size,
            size as f64 / 1024.0
        );

        if let Ok(content) = fs::read_to_string(&path) {
            let lines = content.lines().count();
            let words = content.split_whitespace().count();
            let chars = content.chars().count();
            analysis.push_str(&format!(
                "• Text file: Yes\n• Lines: {}\n• Words: {}\n• Characters: {}\n",
                lines, words, chars
            ));

            let preview_lines: Vec<&str> = content.lines().take(5).collect();
            if !preview_lines.is_empty() {
                analysis.push_str("• Preview (first 5 lines):\n");
                for (i, line) in preview_lines.iter().enumerate() {
                    analysis.push_str(&format!("    {:2}: {}\n", i + 1, line));
                }
            }
        } else {
            analysis.push_str("• Text file: No (Binary or Non-UTF8 data)\n");
        }

        Ok(analysis)
    }
}
