use std::io::{self, Write};
use std::sync::Arc;
use colored::*;

use super::agent_loop::AgentLoop;
use crate::engine::{ChatMessage, EngineHandle};
use crate::hardware::HardwareProfile;
use crate::tools::ToolRegistry;

pub struct AgentTui {
    engine: Arc<EngineHandle>,
    registry: Arc<ToolRegistry>,
    hardware: HardwareProfile,
    agent_loop: AgentLoop,
}

impl AgentTui {
    pub fn new(
        engine: Arc<EngineHandle>,
        registry: Arc<ToolRegistry>,
        hardware: HardwareProfile,
    ) -> Self {
        let agent_loop = AgentLoop::new(Arc::clone(&engine), Arc::clone(&registry));
        Self {
            engine,
            registry,
            hardware,
            agent_loop,
        }
    }

    /// Start the interactive terminal agent session
    pub async fn run_interactive(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.print_welcome_banner();

        let mut history: Vec<ChatMessage> = Vec::new();
        let stdin = io::stdin();

        loop {
            let cwd = self.registry.shell.current_dir();
            let cwd_display = shorten_path(&cwd);

            print!(
                "\n{} {} {}\n{} ",
                "╭─".bright_cyan(),
                "👤 User".bright_green().bold(),
                format!("[{}]", cwd_display).bright_black(),
                "╰❯".bright_cyan()
            );
            io::stdout().flush()?;

            let mut input = String::new();
            if stdin.read_line(&mut input)? == 0 {
                println!("\n{}", "👋 Goodbye from ClawMind!".bright_cyan());
                break;
            }

            let trimmed = input.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Handle slash commands
            if trimmed.starts_with('/') || trimmed == "exit" || trimmed == "quit" || trimmed == "q" {
                if self.handle_command(trimmed, &mut history) {
                    break;
                }
                continue;
            }

            // Run agent turn
            let result = self
                .agent_loop
                .run_turn(
                    trimmed,
                    &mut history,
                    |token| {
                        // Print raw stream if needed or collect
                        let _ = token;
                    },
                    |step_event| {
                        println!();
                        println!(
                            "{}",
                            format!(
                                "╭─ ⚡ ClawMind Autonomous Step [{}/{}] ───────────────────",
                                step_event.step_number, step_event.max_steps
                            )
                            .bright_yellow()
                            .bold()
                        );

                        if let Some(th) = step_event.thought {
                            if !th.is_empty() {
                                println!(
                                    "{} {} {}",
                                    "│".bright_yellow(),
                                    "🧠 Thought:".bright_white().bold(),
                                    th.cyan()
                                );
                            }
                        }

                        println!(
                            "{} {} {}({})",
                            "│".bright_yellow(),
                            "🛠️  Action :".bright_white().bold(),
                            step_event.tool_name.bright_green().bold(),
                            truncate_string(step_event.tool_args, 80).bright_white()
                        );
                    },
                    |tool_result| {
                        let (status_tag, status_color) = if tool_result.success {
                            ("🟢 Success", "green")
                        } else {
                            ("🔴 Failed", "red")
                        };

                        println!(
                            "{} {} {}",
                            "│".bright_yellow(),
                            "📊 Status :".bright_white().bold(),
                            if status_color == "green" {
                                status_tag.bright_green()
                            } else {
                                status_tag.bright_red()
                            }
                        );

                        let lines: Vec<&str> = tool_result.output.lines().collect();
                        if !lines.is_empty() {
                            println!(
                                "{} {}",
                                "│".bright_yellow(),
                                "📋 Output :".bright_white().bold()
                            );
                            let max_show = 6;
                            for line in lines.iter().take(max_show) {
                                println!("{}    {}", "│".bright_yellow(), line.bright_black());
                            }
                            if lines.len() > max_show {
                                println!(
                                    "{}    {}",
                                    "│".bright_yellow(),
                                    format!("(... {} more lines)", lines.len() - max_show).italic().dimmed()
                                );
                            }
                        }

                        println!(
                            "{}",
                            "╰─────────────────────────────────────────────────────────────".bright_yellow()
                        );
                    },
                )
                .await;

            match result {
                Ok(reply) => {
                    self.print_assistant_reply(&reply);
                }
                Err(err) => {
                    println!("\n{} {}", "❌ Error:".bright_red().bold(), err);
                }
            }
        }

        Ok(())
    }

    fn print_welcome_banner(&self) {
        println!();
        println!(
            "{}",
            "╭────────────────────────────────────────────────────────────────────────╮"
                .bright_cyan()
        );
        println!(
            "{}  {}  {}",
            "│".bright_cyan(),
            "🚀 ClawMind Autonomous AI Agent v0.2.0 • Ultra-Fast Native Engine".bright_green().bold(),
            "│".bright_cyan()
        );
        println!(
            "{}  Platform: {:<20} Active Model: {:<22} {}",
            "│".bright_cyan(),
            format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH).bright_white(),
            self.engine.model_name.bright_yellow(),
            "│".bright_cyan()
        );
        println!(
            "{}  Cores: {} Physical ({} Logic)      RAM: {:.1} GB (Active)                  {}",
            "│".bright_cyan(),
            self.hardware.physical_cores.to_string().cyan(),
            self.hardware.logical_threads.to_string().cyan(),
            self.hardware.total_ram_gb,
            "│".bright_cyan()
        );
        println!(
            "{}  Capabilities: Terminal • Filesystem • Google Chrome • Mouse & GUI   {}",
            "│".bright_cyan(),
            "│".bright_cyan()
        );
        println!(
            "{}",
            "╰────────────────────────────────────────────────────────────────────────╯"
                .bright_cyan()
        );
        println!(
            " Type your instruction or question. Type {} for commands, {} to quit.\n",
            "/help".bright_yellow().bold(),
            "/exit".bright_yellow().bold()
        );
    }

    fn print_assistant_reply(&self, reply: &str) {
        let clean_reply = clean_assistant_output(reply);
        println!();
        println!(
            "{} {}",
            "╭─".bright_cyan(),
            "🤖 ClawMind".bright_cyan().bold()
        );

        for line in clean_reply.lines() {
            println!("{}  {}", "│".bright_cyan(), line);
        }

        println!(
            "{}",
            "╰────────────────────────────────────────────────────────────────────────"
                .bright_cyan()
        );
    }

    fn handle_command(&self, cmd: &str, history: &mut Vec<ChatMessage>) -> bool {
        match cmd {
            "/exit" | "exit" | "quit" | "q" => {
                println!("\n{}", "👋 Goodbye from ClawMind!".bright_cyan());
                return true;
            }
            "/clear" | "/c" => {
                print!("\x1B[2J\x1B[1;1H");
                let _ = io::stdout().flush();
                self.print_welcome_banner();
            }
            "/help" | "/h" => {
                println!("\n{}", "📖 ClawMind Agent Commands & Capabilities:".bright_yellow().bold());
                println!("  • {}    : Clear the terminal screen", "/clear".bright_green());
                println!("  • {}  : View indexed client automation scripts (Script-RAG)", "/scripts".bright_green());
                println!("  • {}    : Display all available tools and syntax", "/tools".bright_green());
                println!("  • {}      : Show current working directory", "/cwd".bright_green());
                println!("  • {}   : Show system & engine hardware metrics", "/status".bright_green());
                println!("  • {}    : Reset conversation history", "/reset".bright_green());
                println!("  • {}     : Exit interactive session", "/exit".bright_green());
                println!("\n{}", "💡 What can you ask ClawMind to do?".bright_yellow().bold());
                println!("  - 'run the system health check script'");
                println!("  - 'open chrome and search for movie it'");
                println!("  - 'create a python script in scripts/ to backup files'");
                println!("  - 'list all files in the current directory'");
                println!("  - 'read Cargo.toml and summarize the dependencies'");
                println!("  - 'check available disk space and memory'");
            }
            "/scripts" | "/s" => {
                match self.registry.scripts.list_scripts() {
                    Ok(catalog) => println!("\n{}", catalog),
                    Err(err) => println!("\n{} Failed to list scripts: {}", "⚠️".bright_yellow(), err),
                }
            }
            "/tools" => {
                println!("\n{}", "🛠️  Active Autonomous Tools:".bright_green().bold());
                println!("  1. {}  : Run client automation script (`script_run: backup.sh mydb`)", "script_run".bright_yellow());
                println!("  2. {} : List all client automation scripts with docs", "script_list".bright_yellow());
                println!("  3. {} : Inspect source code of an automation script", "script_inspect".bright_yellow());
                println!("  4. {} : Create a new reusable automation script in `scripts/`", "script_create".bright_yellow());
                println!("  5. {}        : Execute shell/terminal commands (`bash -c`)", "bash".bright_yellow());
                println!("  6. {}  : Create or overwrite files (`{{\"path\": \"...\", \"content\": \"...\"}}`)", "file_write".bright_yellow());
                println!("  7. {}   : Read file contents safely with line limits", "file_read".bright_yellow());
                println!("  8. {} : Delete files or directories safely", "file_delete".bright_yellow());
                println!("  9. {}   : List directory contents with sizes", "file_list".bright_yellow());
                println!(" 10. {} : Inspect file type, line count, and preview", "file_analyze".bright_yellow());
                println!(" 11. {} : Launch Google Chrome directly with search query", "browser_search".bright_yellow());
                println!(" 12. {}   : Open any website visibly in browser", "browser_open".bright_yellow());
                println!(" 13. {}  : Fast headless HTTP page extraction without GUI", "browser_fetch".bright_yellow());
                println!(" 14. {}     : Launch desktop application (`app_open: firefox`)", "app_open".bright_yellow());
                println!(" 15. {}  : Click mouse at coordinates (`{{\"x\": 100, \"y\": 200}}`)", "mouse_click".bright_yellow());
                println!(" 16. {}   : Move mouse cursor to coordinates", "mouse_move".bright_yellow());
                println!(" 17. {}   : Type text into active window", "mouse_type".bright_yellow());
            }
            "/cwd" => {
                let cwd = self.registry.shell.current_dir();
                println!("\n{} {}", "📂 Current Working Directory:".bright_yellow(), cwd.display());
            }
            "/status" => {
                let status = self.engine.status();
                println!("\n{}", "📊 ClawMind Engine Status:".bright_yellow().bold());
                println!("  • State           : {:?}", status);
                println!("  • Model           : {}", self.engine.model_name);
                println!("  • Idle Timeout    : {}s", self.engine.config.idle_timeout_secs);
                println!("  • Prefill Threads : {}", self.engine.config.threads_prefill);
                println!("  • Decode Threads  : {}", self.engine.config.threads_decode);
            }
            "/reset" => {
                history.clear();
                println!("\n{}", "🔄 Conversation history reset.".bright_green());
            }
            _ => {
                println!("\n{} Unknown command '{}'. Type {} for help.", "⚠️".bright_yellow(), cmd, "/help".bright_green());
            }
        }
        false
    }
}

fn shorten_path(path: &std::path::Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::Path::new(&home);
        if let Ok(stripped) = path.strip_prefix(home_path) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

fn truncate_string(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{}...", truncated)
    }
}

fn clean_assistant_output(s: &str) -> String {
    let mut lines = Vec::new();
    for line in s.lines() {
        let trimmed = line.trim();
        let clean = trimmed.trim_start_matches(|c| c == '*' || c == '`' || c == '#');
        if clean.to_lowercase().starts_with("action: done:") {
            lines.push(clean[13..].trim());
        } else if clean.to_lowercase().starts_with("action:") {
            continue;
        } else {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}
