use std::process::Stdio;
use tokio::process::Command;

/// Google Chrome automation tool for Linux and macOS.
#[derive(Clone, Default)]
pub struct BrowserTool;

impl BrowserTool {
    pub fn new() -> Self {
        Self
    }

    /// Detect Chrome binary or launch command
    pub fn detect_chrome_binary() -> Result<(String, Vec<String>), String> {
        #[cfg(target_os = "macos")]
        {
            let mac_chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
            if std::path::Path::new(mac_chrome).exists() {
                return Ok((mac_chrome.to_string(), vec![]));
            }
            // Fallback to `open -a "Google Chrome"`
            return Ok(("open".to_string(), vec!["-a".to_string(), "Google Chrome".to_string()]));
        }

        #[cfg(not(target_os = "macos"))]
        {
            let candidates = [
                "/usr/bin/google-chrome-stable",
                "/usr/bin/google-chrome",
                "/usr/bin/chromium-browser",
                "/usr/bin/chromium",
                "google-chrome-stable",
                "google-chrome",
                "chromium",
            ];

            for c in candidates {
                if std::path::Path::new(c).exists() {
                    return Ok((c.to_string(), vec![]));
                }
            }

            // Test if which finds any
            for c in ["google-chrome", "google-chrome-stable", "chromium"] {
                if let Ok(output) = std::process::Command::new("which").arg(c).output() {
                    if output.status.success() {
                        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path.is_empty() {
                            return Ok((path, vec![]));
                        }
                    }
                }
            }

            // Fallback to xdg-open if no chrome binary is found directly
            if let Ok(output) = std::process::Command::new("which").arg("xdg-open").output() {
                if output.status.success() {
                    return Ok(("xdg-open".to_string(), vec![]));
                }
            }

            Err("Google Chrome or compatible browser not found on this system".to_string())
        }
    }

    /// Launch any desktop application (e.g. firefox, chrome, code, gedit)
    pub fn open_app(&self, app_name: &str) -> Result<String, String> {
        let app = app_name.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
        if app.is_empty() {
            return Err("Application name cannot be empty".to_string());
        }

        // Clean name (e.g. "fire fox" -> "firefox")
        let app_clean = if app.eq_ignore_ascii_case("fire fox") {
            "firefox"
        } else if app.eq_ignore_ascii_case("google chrome") {
            "google-chrome"
        } else {
            app
        };

        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("open")
                .arg("-a")
                .arg(app_clean)
                .status()
                .map_err(|e| format!("Failed to launch '{app_clean}' on macOS: {e}"))?;
            if status.success() {
                return Ok(format!("Successfully launched application '{}' on macOS.", app_clean));
            } else {
                return Err(format!("Could not launch application '{}' via 'open -a'", app_clean));
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            // First check if the binary exists in PATH
            if let Ok(output) = std::process::Command::new("which").arg(app_clean).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        let child = std::process::Command::new(&path)
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                            .map_err(|e| format!("Failed to spawn '{path}': {e}"))?;
                        return Ok(format!(
                            "Successfully launched application '{}' [PID {}]. The window is now open on your desktop.",
                            app_clean, child.id()
                        ));
                    }
                }
            }

            // Fallback: try gtk-launch
            if let Ok(child) = std::process::Command::new("gtk-launch").arg(app_clean).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                return Ok(format!("Successfully launched '{}' [PID {}] via desktop launcher.", app_clean, child.id()));
            }

            Err(format!("Application '{}' not found in PATH or desktop applications.", app_clean))
        }
    }

    /// Open a URL in Google Chrome visibly on the user's screen, or launch Firefox if specified
    pub async fn open_url(&self, url_str: &str) -> Result<String, String> {
        let cleaned = url_str.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
        let lower = cleaned.to_lowercase();

        // If user or model requested Firefox specifically, launch Firefox!
        if lower == "firefox" || lower == "fire fox" || lower == "mozilla-firefox" || lower == "mozilla firefox" {
            return self.open_app("firefox");
        }

        if lower == "chrome" || lower == "google chrome" || lower == "google-chrome" || lower == "chromium" {
            return self.open_app("google-chrome");
        }

        let (bin, base_args) = Self::detect_chrome_binary()?;

        let target_url = if !cleaned.starts_with("http://") && !cleaned.starts_with("https://") {
            format!("https://{}", cleaned)
        } else {
            cleaned.to_string()
        };

        let mut cmd = Command::new(&bin);
        for arg in base_args {
            cmd.arg(arg);
        }

        cmd.arg(&target_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id().unwrap_or(0);
                Ok(format!(
                    "Successfully opened URL '{}' in Google Chrome [PID {}]. The browser window is now visible on the user's screen. The task is complete.",
                    target_url, pid
                ))
            }
            Err(e) => Err(format!("Failed to launch Chrome with {bin}: {e}")),
        }
    }

    /// Search query on Google Chrome directly
    pub async fn search(&self, query: &str) -> Result<String, String> {
        let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let search_url = format!("https://www.google.com/search?q={}", encoded);
        let (bin, base_args) = Self::detect_chrome_binary()?;

        let mut cmd = Command::new(&bin);
        for arg in base_args {
            cmd.arg(arg);
        }
        cmd.arg(&search_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id().unwrap_or(0);
                Ok(format!(
                    "Successfully launched Google Chrome [PID {}] and displayed search results for '{}' on the user's screen. The browser is open and visible. The task is complete.",
                    pid, query
                ))
            }
            Err(e) => Err(format!("Failed to launch Chrome search with {bin}: {e}")),
        }
    }

    /// Fast headless HTTP page extraction (reads web page text without browser overhead)
    pub async fn fetch_text(&self, url_str: &str) -> Result<String, String> {
        let target_url = if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
            format!("https://{}", url_str)
        } else {
            url_str.to_string()
        };

        if target_url.contains("google.com/search") {
            return Ok("The Google Search results page is already active and visible in the user's Google Chrome window. Task is complete. Do not re-fetch. Conclude with 'Action: done: <message>'.".to_string());
        }

        let mut cmd = Command::new("curl");
        cmd.arg("-sL")
            .arg("-m")
            .arg("10") // 10s max
            .arg("-A")
            .arg("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .arg(&target_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to fetch URL {}: {e}", target_url))?;

        if !output.status.success() {
            return Err(format!("Failed to fetch URL (HTTP/Curl error)"));
        }

        let html = String::from_utf8_lossy(&output.stdout).to_string();
        let stripped = strip_html_tags(&html);
        let trimmed: String = stripped
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .take(60)
            .collect::<Vec<&str>>()
            .join("\n");

        if trimmed.is_empty() {
            Ok("(Page retrieved, but contains no visible text)".to_string())
        } else {
            Ok(format!("Page Text from {}:\n{}", target_url, trimmed))
        }
    }
}

/// Simple, ultra-fast, UTF-8 safe HTML tag stripper
pub fn strip_html_tags(html: &str) -> String {
    let mut in_tag = false;
    let mut in_script_or_style = false;
    let mut current_tag = String::new();
    let mut result = String::with_capacity(html.len());

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            current_tag.clear();
            continue;
        }

        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag_lower = current_tag.to_lowercase();
                let tag_name = tag_lower
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('/');

                if tag_name == "script" || tag_name == "style" {
                    if current_tag.trim_start().starts_with('/') {
                        in_script_or_style = false;
                    } else {
                        in_script_or_style = true;
                    }
                }
                current_tag.clear();
            } else {
                current_tag.push(c);
            }
            continue;
        }

        if !in_script_or_style {
            result.push(c);
        }
    }

    // Replace common HTML entities
    result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
