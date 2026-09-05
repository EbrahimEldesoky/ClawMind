use std::sync::Arc;
use serde_json::Value;

use crate::engine::{ChatMessage, EngineHandle, InferResponse};
use crate::tools::{clean_special_tokens, ToolCall, ToolRegistry, ToolResult};
use super::prompts::build_system_prompt;

pub struct AgentStepEvent<'a> {
    pub step_number: usize,
    pub max_steps: usize,
    pub thought: Option<&'a str>,
    pub tool_name: &'a str,
    pub tool_args: &'a str,
}

pub struct AgentLoop {
    engine: Arc<EngineHandle>,
    registry: Arc<ToolRegistry>,
    max_steps: usize,
}

impl AgentLoop {
    pub fn new(engine: Arc<EngineHandle>, registry: Arc<ToolRegistry>) -> Self {
        Self {
            engine,
            registry,
            max_steps: 8,
        }
    }

    /// Execute a task or user message through the autonomous ReAct agent loop
    pub async fn run_turn<FToken, FToolStart, FToolEnd>(
        &self,
        user_input: &str,
        history: &mut Vec<ChatMessage>,
        mut on_token: FToken,
        mut on_tool_start: FToolStart,
        mut on_tool_end: FToolEnd,
    ) -> Result<String, String>
    where
        FToken: FnMut(&str),
        FToolStart: FnMut(&AgentStepEvent),
        FToolEnd: FnMut(&ToolResult),
    {
        let current_dir = self.registry.shell.current_dir();
        let rag_section = self.registry.scripts.format_rag_prompt();
        let system_prompt = build_system_prompt(&current_dir, &rag_section);

        // Ensure system prompt is present at the beginning of history
        if history.is_empty() || history[0].role != "system" {
            history.insert(
                0,
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(crate::engine::MessageContent::Text(system_prompt)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        } else {
            // Update CWD and dynamic Script-RAG catalog in system prompt dynamically
            history[0].content = Some(crate::engine::MessageContent::Text(system_prompt));
        }

        // Add user message
        history.push(ChatMessage {
            role: "user".to_string(),
            content: Some(crate::engine::MessageContent::Text(user_input.to_string())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });

        let mut step = 0;
        let mut final_answer = String::new();
        let mut executed_tools: Vec<(String, String)> = Vec::new();

        while step < self.max_steps {
            step += 1;

            // Format history into Gemma chat template
            // Keep recent history within bounds to preserve CPU prefill speed
            let bounded_history = trim_history(history, 8);
            let prompt = EngineHandle::format_messages_to_prompt(&bounded_history);

            let mut rx = self.engine.infer(prompt, 512, 0.6, 0.95).await;
            let mut generated_text = String::new();

            while let Some(res) = rx.recv().await {
                match res {
                    InferResponse::Token(token) => {
                        generated_text.push_str(&token);
                        on_token(&token);
                    }
                    InferResponse::Done { .. } => break,
                    InferResponse::Error(err) => return Err(format!("Inference engine error: {err}")),
                }
            }

            // Check if generation contains an action
            if let Some(tool_call) = parse_action(&generated_text) {
                // If the tool is "done", conclude the loop
                if tool_call.name == "done" {
                    final_answer = tool_call.raw_args;
                    history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(crate::engine::MessageContent::Text(generated_text)),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    break;
                }

                let call_key = (tool_call.name.clone(), tool_call.raw_args.clone());

                // Prevent infinite loop on duplicate tool invocation
                if executed_tools.contains(&call_key) {
                    final_answer = format!("Action '{}' has already been performed successfully.", tool_call.name);
                    break;
                }

                let is_browser_or_app = tool_call.name == "browser_search"
                    || tool_call.name == "browser_open"
                    || tool_call.name == "app_open"
                    || tool_call.name == "launch_app";
                executed_tools.push(call_key);

                let step_event = AgentStepEvent {
                    step_number: step,
                    max_steps: self.max_steps,
                    thought: extract_thought(&generated_text),
                    tool_name: &tool_call.name,
                    tool_args: &tool_call.raw_args,
                };
                on_tool_start(&step_event);

                // Execute tool
                let tool_result = self.registry.execute(&tool_call).await;
                on_tool_end(&tool_result);

                // For browser launch or application launch actions, once open on desktop, task is complete!
                if is_browser_or_app && tool_result.success {
                    final_answer = tool_result.output.clone();
                    history.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: Some(crate::engine::MessageContent::Text(generated_text)),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    break;
                }

                // Append assistant turn and tool observation turn
                history.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(crate::engine::MessageContent::Text(generated_text)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });

                let obs_content = format!("Observation: {}", tool_result.output);
                history.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(crate::engine::MessageContent::Text(obs_content)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
            } else {
                // No action parsed, treat whole generation as direct answer
                final_answer = generated_text.clone();
                history.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(crate::engine::MessageContent::Text(generated_text)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
                break;
            }
        }

        if final_answer.is_empty() {
            final_answer = "Task execution completed.".to_string();
        }

        Ok(final_answer)
    }
}

/// Trims history preserving system message and last N interaction turns
fn trim_history(history: &[ChatMessage], keep_recent: usize) -> Vec<ChatMessage> {
    if history.len() <= keep_recent + 1 {
        return history.to_vec();
    }

    let mut result = Vec::new();
    if let Some(first) = history.first() {
        if first.role == "system" {
            result.push(first.clone());
        }
    }

    let start_idx = history.len().saturating_sub(keep_recent);
    for item in &history[start_idx..] {
        if item.role != "system" {
            result.push(item.clone());
        }
    }
    result
}

/// Parses an Action from model generation (multiline, JSON, parenthesis, and code-block aware)
pub fn parse_action(text: &str) -> Option<ToolCall> {
    let clean = clean_special_tokens(text);
    let lower = clean.to_lowercase();

    // Find "Action" followed optionally by spaces and ":"
    let after_action_opt = if let Some(pos) = lower.find("action") {
        let rest = &clean[pos + 6..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with(':') {
            Some(trimmed[1..].trim())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(after_action) = after_action_opt {
        // 1. Look for code block after action first:
        // e.g. Action: bash\n```bash\ncommand\n``` or Action: file_write: main.rs\n```rust\n...```
        if let Some(code_start) = after_action.find("```") {
            let before_code = after_action[..code_start].trim();
            let rest = &after_action[code_start + 3..];
            let lang_line_end = rest.find('\n').unwrap_or(rest.len());
            let lang = rest[..lang_line_end].trim();
            let body = &rest[lang_line_end..];
            let code = if let Some(code_end) = body.find("```") {
                &body[..code_end]
            } else {
                body
            };

            // If before_code has tool and path (e.g. "file_write: main.rs" or "file_write")
            let (tool, target_or_args) = if let Some((t, a)) = before_code.split_once(':') {
                (t.trim(), a.trim())
            } else {
                (before_code.trim(), "")
            };

            let tool_name = if !tool.is_empty() {
                tool
            } else if !lang.is_empty() {
                lang
            } else {
                "bash"
            };

            let raw_args = if tool_name == "file_write" || tool_name == "write_file" {
                if !target_or_args.is_empty() {
                    format!("{}\n```\n{}\n```", target_or_args, code.trim())
                } else {
                    code.trim().to_string()
                }
            } else {
                code.trim().to_string()
            };

            return Some(ToolCall {
                name: tool_name.to_string(),
                raw_args,
            });
        }

        // 2. Look for `Action: <tool>(<arguments>)` (e.g. browser_search("movie it") or mouse_click({"x": 500, "y": 500}))
        if let Some(paren_open) = after_action.find('(') {
            let potential_tool = after_action[..paren_open].trim().trim_start_matches(':').trim();
            if !potential_tool.is_empty()
                && potential_tool.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                if let Some(paren_close) = after_action.rfind(')') {
                    if paren_close > paren_open {
                        let inner_args = &after_action[paren_open + 1..paren_close];
                        return Some(ToolCall {
                            name: potential_tool.to_string(),
                            raw_args: inner_args.trim().to_string(),
                        });
                    }
                }
            }
        }

        // 3. Look for `Action: <tool_name>: <arguments>` (multiline & JSON aware)
        if let Some((tool, rest_args)) = after_action.split_once(':') {
            let tool_name = tool.trim().trim_matches(|c| c == '*' || c == '`' || c == '#');
            if !tool_name.is_empty()
                && tool_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                let args_trimmed = rest_args.trim();

                // If args starts with JSON '{', find matching closing '}'
                let full_args = if args_trimmed.starts_with('{') {
                    if let Some(matching_brace) = find_matching_brace(args_trimmed) {
                        &args_trimmed[..=matching_brace]
                    } else {
                        args_trimmed.lines().next().unwrap_or(args_trimmed)
                    }
                } else {
                    let end_pos = args_trimmed
                        .find("\n\n")
                        .or_else(|| args_trimmed.find("\nAction:"))
                        .or_else(|| args_trimmed.find("\nObservation:"))
                        .unwrap_or(args_trimmed.len());
                    &args_trimmed[..end_pos]
                };

                return Some(ToolCall {
                    name: tool_name.to_string(),
                    raw_args: full_args.trim().to_string(),
                });
            }
        }

        // 4. Look for `Action: <tool_name> <arguments>`
        let parts: Vec<&str> = after_action.split_whitespace().collect();
        if parts.len() > 1 {
            let tool_name = parts[0].trim_matches(':');
            if tool_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let args = after_action[parts[0].len()..].trim();
                let end_pos = args.find("\n\n").unwrap_or(args.len());
                return Some(ToolCall {
                    name: tool_name.to_string(),
                    raw_args: args[..end_pos].trim().to_string(),
                });
            }
        }
    }

    // 5. Look for JSON format {"action": "...", ...} in the text
    if let Some(start) = clean.find('{') {
        let slice = &clean[start..];
        if let Some(matching) = find_matching_brace(slice) {
            let json_candidate = &slice[..=matching];
            if let Ok(v) = serde_json::from_str::<Value>(json_candidate) {
                if let Some(action) = v.get("action").and_then(|x| x.as_str()) {
                    return Some(ToolCall {
                        name: action.to_string(),
                        raw_args: json_candidate.to_string(),
                    });
                }
            }
        }
    }

    None
}

/// Helper to find index of closing brace matching the opening brace at index 0
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_str = false;
    let mut escape = false;

    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Extract optional thought before action
fn extract_thought(text: &str) -> Option<&str> {
    for line in text.lines() {
        let trimmed = line.trim();
        let clean = trimmed.trim_start_matches(|c| c == '*' || c == '`' || c == '#');
        if clean.to_lowercase().starts_with("thought:") {
            return Some(clean[8..].trim());
        }
    }
    None
}
