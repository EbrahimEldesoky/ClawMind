use std::process::Command;

/// Mouse and Keyboard GUI automation tool for Linux and macOS.
#[derive(Clone, Default)]
pub struct MouseTool;

impl MouseTool {
    pub fn new() -> Self {
        Self
    }

    /// Click mouse button at (x, y) coordinates
    pub fn click(&self, x: i32, y: i32, button: u8) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "tell application \"System Events\" to click at {{{}, {}}}",
                x, y
            );
            let status = Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .status()
                .map_err(|e| format!("osascript failed: {e}"))?;
            if status.success() {
                return Ok(format!("Clicked at ({}, {}) with button {}", x, y, button));
            } else {
                return Err("Failed to click via osascript".to_string());
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Try xdotool
            if command_exists("xdotool") {
                let btn = button.to_string();
                let output = Command::new("xdotool")
                    .arg("mousemove")
                    .arg(x.to_string())
                    .arg(y.to_string())
                    .arg("click")
                    .arg(&btn)
                    .output()
                    .map_err(|e| format!("xdotool failed: {e}"))?;
                if output.status.success() {
                    return Ok(format!("Clicked at ({}, {}) with button {}", x, y, button));
                }
            }

            // Fallback: Check if python3 with pyautogui or pynput exists
            let py_script = format!(
                "try:\n    import pyautogui\n    pyautogui.click({}, {})\n    print('OK')\nexcept Exception as e:\n    print(f'ERR:{{e}}')",
                x, y
            );
            if let Ok(output) = Command::new("python3").arg("-c").arg(&py_script).output() {
                let out = String::from_utf8_lossy(&output.stdout);
                if out.contains("OK") {
                    return Ok(format!("Clicked at ({}, {}) via pyautogui", x, y));
                }
            }

            Err("Mouse click requires 'xdotool' or 'pyautogui' installed on Linux. (Run: sudo apt install xdotool)".to_string())
        }
    }

    /// Move mouse cursor to (x, y)
    pub fn move_to(&self, x: i32, y: i32) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "tell application \"System Events\" to set position of (UI element 1) to {{{}, {}}}",
                x, y
            );
            let _ = Command::new("osascript").arg("-e").arg(&script).output();
            Ok(format!("Moved mouse to ({}, {})", x, y))
        }

        #[cfg(not(target_os = "macos"))]
        {
            if command_exists("xdotool") {
                let output = Command::new("xdotool")
                    .arg("mousemove")
                    .arg(x.to_string())
                    .arg(y.to_string())
                    .output()
                    .map_err(|e| format!("xdotool failed: {e}"))?;
                if output.status.success() {
                    return Ok(format!("Moved mouse to ({}, {})", x, y));
                }
            }

            let py_script = format!(
                "try:\n    import pyautogui\n    pyautogui.moveTo({}, {})\n    print('OK')\nexcept Exception as e:\n    print(f'ERR:{{e}}')",
                x, y
            );
            if let Ok(output) = Command::new("python3").arg("-c").arg(&py_script).output() {
                let out = String::from_utf8_lossy(&output.stdout);
                if out.contains("OK") {
                    return Ok(format!("Moved mouse to ({}, {})", x, y));
                }
            }

            Err("Mouse move requires 'xdotool' or 'pyautogui' installed on Linux. (Run: sudo apt install xdotool)".to_string())
        }
    }

    /// Type text into active window
    pub fn type_text(&self, text: &str) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                "tell application \"System Events\" to keystroke \"{}\"",
                escaped
            );
            let status = Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .status()
                .map_err(|e| format!("osascript failed: {e}"))?;
            if status.success() {
                return Ok(format!("Typed {} characters into active window", text.len()));
            }
            return Err("Failed to type text via osascript".to_string());
        }

        #[cfg(not(target_os = "macos"))]
        {
            if command_exists("xdotool") {
                let output = Command::new("xdotool")
                    .arg("type")
                    .arg("--clearmodifiers")
                    .arg(text)
                    .output()
                    .map_err(|e| format!("xdotool failed: {e}"))?;
                if output.status.success() {
                    return Ok(format!("Typed {} characters into active window", text.len()));
                }
            }

            let py_script = format!(
                "try:\n    import pyautogui\n    pyautogui.write('''{}''')\n    print('OK')\nexcept Exception as e:\n    print(f'ERR:{{e}}')",
                text.replace('\'', "\\'")
            );
            if let Ok(output) = Command::new("python3").arg("-c").arg(&py_script).output() {
                let out = String::from_utf8_lossy(&output.stdout);
                if out.contains("OK") {
                    return Ok(format!("Typed {} characters into active window", text.len()));
                }
            }

            Err("Typing requires 'xdotool' or 'pyautogui' installed on Linux. (Run: sudo apt install xdotool)".to_string())
        }
    }

    /// Press a special key (e.g. Return, Tab, Escape)
    pub fn press_key(&self, key: &str) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            let script = match key.to_lowercase().as_str() {
                "return" | "enter" => "tell application \"System Events\" to key code 36",
                "escape" | "esc" => "tell application \"System Events\" to key code 53",
                "tab" => "tell application \"System Events\" to key code 48",
                "space" => "tell application \"System Events\" to key code 49",
                _ => return Err(format!("Unsupported special key for macOS: {key}")),
            };
            let status = Command::new("osascript")
                .arg("-e")
                .arg(script)
                .status()
                .map_err(|e| format!("osascript failed: {e}"))?;
            if status.success() {
                return Ok(format!("Pressed key: {key}"));
            }
            return Err("Failed to press key via osascript".to_string());
        }

        #[cfg(not(target_os = "macos"))]
        {
            if command_exists("xdotool") {
                let output = Command::new("xdotool")
                    .arg("key")
                    .arg(key)
                    .output()
                    .map_err(|e| format!("xdotool failed: {e}"))?;
                if output.status.success() {
                    return Ok(format!("Pressed key: {key}"));
                }
            }
            Err("Key press requires 'xdotool' installed on Linux. (Run: sudo apt install xdotool)".to_string())
        }
    }
}

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
