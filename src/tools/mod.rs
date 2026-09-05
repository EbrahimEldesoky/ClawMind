pub mod browser;
pub mod fs;
pub mod mouse;
pub mod scripts;
pub mod shell;

use std::sync::Arc;
use serde_json::Value;

pub use browser::BrowserTool;
pub use fs::FsTool;
pub use mouse::MouseTool;
pub use scripts::{ScriptsTool, ScriptInfo};
pub use shell::ShellTool;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub raw_args: String,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_name: String,
    pub success: bool,
    pub output: String,
}

/// Central registry managing all executable agent tools
#[derive(Clone)]
pub struct ToolRegistry {
    pub shell: Arc<ShellTool>,
    pub fs: Arc<FsTool>,
    pub browser: Arc<BrowserTool>,
    pub mouse: Arc<MouseTool>,
    pub scripts: Arc<ScriptsTool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            shell: Arc::new(ShellTool::new()),
            fs: Arc::new(FsTool::new()),
            browser: Arc::new(BrowserTool::new()),
            mouse: Arc::new(MouseTool::new()),
            scripts: Arc::new(ScriptsTool::new()),
        }
    }

    /// Execute a tool call and return formatted result
    pub async fn execute(&self, call: &ToolCall) -> ToolResult {
        let name = call.name.trim().to_lowercase();
        let args = call.raw_args.trim();

        let current_dir = self.shell.current_dir();

        match name.clone().as_str() {
            // 1. Shell commands
            "bash" | "sh" | "shell" | "terminal" | "exec" => {
                match self.shell.execute(args).await {
                    Ok(output) => ToolResult {
                        tool_name: name,
                        success: true,
                        output,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 2. Filesystem write
            "file_write" | "write_file" | "create_file" => {
                let (path, content) = match parse_file_write_args(args) {
                    Ok(pair) => pair,
                    Err(err) => {
                        return ToolResult {
                            tool_name: name,
                            success: false,
                            output: err,
                        };
                    }
                };

                match self.fs.write_file(&current_dir, &path, &content) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 3. Filesystem read
            "file_read" | "read_file" => {
                let path = parse_single_arg_or_json(args, "path");
                let max_lines = parse_json_field(args, "max_lines").and_then(|s| s.parse::<usize>().ok());

                if path.is_empty() || path.starts_with('{') {
                    return ToolResult {
                        tool_name: name,
                        success: false,
                        output: format!("Invalid path provided: '{}'", path),
                    };
                }

                match self.fs.read_file(&current_dir, &path, max_lines) {
                    Ok(content) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: content,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 4. Filesystem delete
            "file_delete" | "delete_file" | "remove_file" => {
                let path = parse_single_arg_or_json(args, "path");
                match self.fs.delete_path(&current_dir, &path) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 5. Filesystem list
            "file_list" | "list_dir" | "ls" => {
                let path = parse_single_arg_or_json(args, "path");
                let path_to_list = if path.is_empty() { "." } else { &path };
                match self.fs.list_dir(&current_dir, path_to_list) {
                    Ok(contents) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: contents,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 6. Filesystem analyze
            "file_analyze" | "analyze_file" => {
                let path = parse_single_arg_or_json(args, "path");
                match self.fs.analyze_file(&current_dir, &path) {
                    Ok(analysis) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: analysis,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 7. Chrome search
            "browser_search" | "google_search" | "search" => {
                let query = parse_single_arg_or_json(args, "query");
                match self.browser.search(&query).await {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 8. Chrome open URL
            "browser_open" | "open_url" | "open_browser" => {
                let url = parse_single_arg_or_json(args, "url");
                match self.browser.open_url(&url).await {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 9. Fast headless fetch
            "browser_fetch" | "fetch_url" | "web_fetch" => {
                let url = parse_single_arg_or_json(args, "url");
                match self.browser.fetch_text(&url).await {
                    Ok(text) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: text,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 10. Mouse Click
            "mouse_click" => {
                let x = parse_json_field(args, "x").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                let y = parse_json_field(args, "y").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                let button = parse_json_field(args, "button").and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
                match self.mouse.click(x, y, button) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 11. Mouse Move
            "mouse_move" => {
                let x = parse_json_field(args, "x").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                let y = parse_json_field(args, "y").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
                match self.mouse.move_to(x, y) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 12. Mouse / Keyboard Type
            "mouse_type" | "type_text" => {
                let text = parse_single_arg_or_json(args, "text");
                match self.mouse.type_text(&text) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 13. Special Key Press
            "mouse_key" | "press_key" => {
                let key = parse_single_arg_or_json(args, "key");
                match self.mouse.press_key(&key) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 14. Launch Desktop Application (firefox, chrome, etc.)
            "app_open" | "launch_app" | "open_app" => {
                let app = parse_single_arg_or_json(args, "name");
                let target = if app.is_empty() {
                    parse_single_arg_or_json(args, "app")
                } else {
                    app
                };
                let target_app = if target.is_empty() { args.trim() } else { &target };
                match self.browser.open_app(target_app) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 15. Execute Client Automation Script
            "script_run" | "run_script" | "exec_script" => {
                let (script_name, script_args) = parse_script_run_args(args);
                match self.scripts.execute_script(&script_name, &script_args).await {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 16. List Client Automation Scripts
            "script_list" | "list_scripts" => {
                match self.scripts.list_scripts() {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 17. Inspect Client Automation Script
            "script_inspect" | "inspect_script" | "read_script" => {
                let script_name = parse_single_arg_or_json(args, "name");
                let target_name = if script_name.is_empty() {
                    parse_single_arg_or_json(args, "script")
                } else {
                    script_name
                };
                let final_name = if target_name.is_empty() { args.trim() } else { &target_name };
                match self.scripts.inspect_script(final_name) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            // 18. Create New Automation Script
            "script_create" | "create_script" => {
                let (script_name, content) = match parse_script_create_args(args) {
                    Ok(pair) => pair,
                    Err(err) => {
                        return ToolResult {
                            tool_name: name,
                            success: false,
                            output: err,
                        };
                    }
                };
                match self.scripts.create_script(&script_name, &content) {
                    Ok(msg) => ToolResult {
                        tool_name: name,
                        success: true,
                        output: msg,
                    },
                    Err(err) => ToolResult {
                        tool_name: name,
                        success: false,
                        output: err,
                    },
                }
            }

            unknown => ToolResult {
                tool_name: name,
                success: false,
                output: format!(
                    "Unknown tool '{}'. Available: script_run, script_list, script_inspect, script_create, bash, file_write, file_read, file_delete, file_list, file_analyze, browser_search, browser_open, browser_fetch, app_open, mouse_click, mouse_move, mouse_type",
                    unknown
                ),
            },
        }
    }
}

/// Strip special model tokens like <tool_call|>, <|im_end|>, etc.
pub fn clean_special_tokens(s: &str) -> String {
    let tokens = [
        "<tool_call|>",
        "<tool_call>",
        "</tool_call>",
        "<|im_end|>",
        "<|im_start|>",
        "<|turn|>",
        "<turn|>",
        "<end_of_turn>",
        "<|end_of_turn|>",
        "<eos>",
        "<|eot_id|>",
    ];
    let mut res = s.to_string();
    for tok in tokens {
        res = res.replace(tok, "");
    }
    while let Some(start) = res.find("<call:") {
        if let Some(end) = res[start..].find('>') {
            res.replace_range(start..start + end + 1, "");
        } else {
            break;
        }
    }
    res = res.replace("</call>", "");
    res
}

/// Extract a string field from relaxed or malformed JSON
pub fn extract_json_field_relaxed(json_str: &str, field: &str) -> Option<String> {
    let lower = json_str.to_lowercase();
    let target_double = format!("\"{}\"", field.to_lowercase());
    let target_single = format!("'{}'", field.to_lowercase());

    let field_pos = lower.find(&target_double).or_else(|| lower.find(&target_single))?;
    let after_field = &json_str[field_pos..];
    let colon_pos = after_field.find(':')?;
    let value_part = after_field[colon_pos + 1..].trim_start();

    if value_part.is_empty() {
        return None;
    }

    let first_char = value_part.chars().next()?;
    if first_char == '"' || first_char == '\'' {
        let quote = first_char;
        let mut value = String::new();
        let mut escaped = false;

        for c in value_part[1..].chars() {
            if escaped {
                match c {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '\\' => value.push('\\'),
                    '\'' => value.push('\''),
                    '"' => value.push('"'),
                    _ => {
                        value.push('\\');
                        value.push(c);
                    }
                }
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                return Some(value);
            } else {
                value.push(c);
            }
        }
        if !value.is_empty() {
            return Some(value);
        }
    } else {
        let end_pos = value_part
            .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(value_part.len());
        let val = value_part[..end_pos].trim();
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }

    None
}

/// Robust parser for file_write tool arguments
pub fn parse_file_write_args(args: &str) -> Result<(String, String), String> {
    let clean = clean_special_tokens(args);
    let trimmed = clean.trim();

    // 1. Check for markdown code block format:
    // e.g. path/to/file.rs\n```rust\ncontent\n```
    // or {"path": "main.rs"}\n```rust\ncontent\n```
    if let Some(cb_start) = trimmed.find("```") {
        let before_cb = trimmed[..cb_start].trim();
        let path = parse_single_arg_or_json(before_cb, "path");

        let after_cb = &trimmed[cb_start + 3..];
        let lang_end = after_cb.find('\n').unwrap_or(after_cb.len());
        let rest = &after_cb[lang_end..];
        let content = if let Some(cb_end) = rest.find("```") {
            &rest[..cb_end]
        } else {
            rest
        };

        if !path.is_empty() && !path.starts_with('{') {
            return Ok((path, content.trim_matches('\n').to_string()));
        }
    }

    // 2. Check for JSON format
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let json_slice = &trimmed[start..=end];
            // Try strict JSON first
            if let Ok(v) = serde_json::from_str::<Value>(json_slice) {
                let p = v.get("path").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                let c = v.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if !p.is_empty() && !p.starts_with('{') {
                    return Ok((p, c));
                }
            }

            // Fallback: relaxed extraction
            let path_opt = extract_json_field_relaxed(json_slice, "path");
            let content_opt = extract_json_field_relaxed(json_slice, "content");
            if let Some(p) = path_opt {
                let p_clean = p.trim().to_string();
                if !p_clean.is_empty() && !p_clean.starts_with('{') {
                    let c = content_opt.unwrap_or_default();
                    return Ok((p_clean, c));
                }
            }
        }
    }

    // 3. Plain text format (NOT starting with {): path: content
    if !trimmed.starts_with('{') {
        if let Some((p, c)) = trimmed.split_once(':') {
            let p_trim = p.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
            if !p_trim.is_empty()
                && !p_trim.contains('{')
                && !p_trim.contains('}')
                && !p_trim.contains('"')
            {
                return Ok((p_trim.to_string(), c.trim().to_string()));
            }
        }
    }

    Err(format!(
        "Failed to parse file_write arguments. Expected JSON: {{\"path\": \"...\", \"content\": \"...\"}} or path followed by code block. Received: {}",
        if trimmed.len() > 100 { &trimmed[..100] } else { trimmed }
    ))
}

/// Helper to parse single string argument either from plain string or JSON
pub fn parse_single_arg_or_json(args: &str, field: &str) -> String {
    let clean = clean_special_tokens(args);
    let trimmed = clean.trim();

    // Check if contains JSON block
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let json_slice = &trimmed[start..=end];
            if let Ok(v) = serde_json::from_str::<Value>(json_slice) {
                if let Some(s) = v.get(field).and_then(|x| x.as_str()) {
                    let res = s.trim().to_string();
                    if !res.is_empty() && !res.starts_with('{') {
                        return res;
                    }
                }
            }
            if let Some(s) = extract_json_field_relaxed(json_slice, field) {
                let res = s.trim().to_string();
                if !res.is_empty() && !res.starts_with('{') {
                    return res;
                }
            }
        }
    }

    // Strip leading field name if formatted like "path: myfile.txt"
    let without_prefix = if let Some(colon) = trimmed.find(':') {
        let prefix = trimmed[..colon].trim().to_lowercase();
        if prefix == field.to_lowercase() {
            trimmed[colon + 1..].trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    // Remove surrounding quotes and brackets if any
    let stripped = without_prefix.trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '(' || c == ')');
    stripped.to_string()
}

/// Helper to parse a field from a JSON string
pub fn parse_json_field(args: &str, field: &str) -> Option<String> {
    let clean = clean_special_tokens(args);
    let trimmed = clean.trim();

    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let json_slice = &trimmed[start..=end];
            if let Ok(v) = serde_json::from_str::<Value>(json_slice) {
                if let Some(val) = v.get(field) {
                    if let Some(s) = val.as_str() {
                        return Some(s.to_string());
                    } else if let Some(n) = val.as_i64() {
                        return Some(n.to_string());
                    } else if let Some(b) = val.as_bool() {
                        return Some(b.to_string());
                    }
                }
            }
            if let Some(s) = extract_json_field_relaxed(json_slice, field) {
                return Some(s);
            }
        }
    }
    None
}

/// Helper to parse script_run arguments into (script_name, raw_args)
pub fn parse_script_run_args(args: &str) -> (String, String) {
    let clean = clean_special_tokens(args);
    let trimmed = clean.trim();

    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            if let Ok(v) = serde_json::from_str::<Value>(&trimmed[start..=end]) {
                let name = v.get("name")
                    .or_else(|| v.get("script"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                let raw_args = if let Some(args_val) = v.get("args") {
                    if let Some(arr) = args_val.as_array() {
                        arr.iter()
                            .filter_map(|x| {
                                if let Some(s) = x.as_str() {
                                    Some(s.to_string())
                                } else if let Some(n) = x.as_i64() {
                                    Some(n.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else if let Some(s) = args_val.as_str() {
                        s.to_string()
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                };

                if !name.is_empty() {
                    return (name, raw_args);
                }
            }
        }
    }

    // Fallback: first token is script name, rest are arguments
    if let Some((name, rest)) = trimmed.split_once(' ') {
        (name.trim().trim_matches('"').trim_matches('\'').to_string(), rest.trim().to_string())
    } else {
        (trimmed.trim_matches('"').trim_matches('\'').to_string(), "".to_string())
    }
}

/// Helper to parse script_create arguments into (script_name, content)
pub fn parse_script_create_args(args: &str) -> Result<(String, String), String> {
    let clean = clean_special_tokens(args);
    let trimmed = clean.trim();

    // 1. Check for markdown code blocks
    if let Some(cb_start) = trimmed.find("```") {
        let before_cb = trimmed[..cb_start].trim();
        let name = parse_single_arg_or_json(before_cb, "name");
        let name = if name.is_empty() {
            parse_single_arg_or_json(before_cb, "path")
        } else {
            name
        };

        let after_cb = &trimmed[cb_start + 3..];
        let lang_end = after_cb.find('\n').unwrap_or(after_cb.len());
        let rest = &after_cb[lang_end..];
        let content = if let Some(cb_end) = rest.find("```") {
            &rest[..cb_end]
        } else {
            rest
        };

        if !name.is_empty() && !name.starts_with('{') {
            return Ok((name, content.trim_matches('\n').to_string()));
        }
    }

    // 2. Check JSON
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let json_slice = &trimmed[start..=end];
            if let Ok(v) = serde_json::from_str::<Value>(json_slice) {
                let name = v.get("name")
                    .or_else(|| v.get("path"))
                    .or_else(|| v.get("script"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if !name.is_empty() {
                    return Ok((name, content));
                }
            }

            let name_opt = extract_json_field_relaxed(json_slice, "name")
                .or_else(|| extract_json_field_relaxed(json_slice, "path"));
            let content_opt = extract_json_field_relaxed(json_slice, "content");
            if let Some(name) = name_opt {
                let name_clean = name.trim().to_string();
                if !name_clean.is_empty() {
                    return Ok((name_clean, content_opt.unwrap_or_default()));
                }
            }
        }
    }

    Err(format!(
        "Failed to parse script_create arguments. Expected JSON: {{\"name\": \"...\", \"content\": \"...\"}} or name followed by code block."
    ))
}
