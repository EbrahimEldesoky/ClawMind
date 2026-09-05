use std::path::PathBuf;
use clawmind::agent::parse_action;
use clawmind::tools::{FsTool, ShellTool};

#[tokio::test]
async fn test_shell_tool_echo() {
    let shell = ShellTool::new();
    let result = shell.execute("echo 'Hello ClawMind'").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().trim(), "Hello ClawMind");
}

#[tokio::test]
async fn test_shell_tool_dir_tracking() {
    let shell = ShellTool::new();
    let initial_dir = shell.current_dir();
    let cd_res = shell.execute("cd /tmp").await;
    assert!(cd_res.is_ok());
    assert_eq!(shell.current_dir(), PathBuf::from("/tmp"));
    shell.set_current_dir(initial_dir);
}

#[tokio::test]
async fn test_fs_tool_operations() {
    let fs = FsTool::new();
    let temp_dir = std::env::temp_dir();
    let test_file = "clawmind_test_file.txt";

    // Write
    let write_res = fs.write_file(&temp_dir, test_file, "Line 1\nLine 2\nLine 3");
    assert!(write_res.is_ok());

    // Read
    let read_res = fs.read_file(&temp_dir, test_file, Some(2));
    assert!(read_res.is_ok());
    let content = read_res.unwrap();
    assert!(content.contains("Line 1"));

    // Analyze
    let analyze_res = fs.analyze_file(&temp_dir, test_file);
    assert!(analyze_res.is_ok());
    assert!(analyze_res.unwrap().contains("Lines: 3"));

    // Delete
    let del_res = fs.delete_path(&temp_dir, test_file);
    assert!(del_res.is_ok());
}

#[test]
fn test_parse_action_formats() {
    // 1. Colon format
    let action1 = parse_action("Thought: I need to check files\nAction: bash: ls -la");
    assert!(action1.is_some());
    let a1 = action1.unwrap();
    assert_eq!(a1.name, "bash");
    assert_eq!(a1.raw_args, "ls -la");

    // 2. Browser search
    let action2 = parse_action("Action: browser_search: movie it");
    assert!(action2.is_some());
    let a2 = action2.unwrap();
    assert_eq!(a2.name, "browser_search");
    assert_eq!(a2.raw_args, "movie it");

    // 3. JSON format
    let action3 = parse_action("{\"action\": \"file_write\", \"path\": \"test.txt\", \"content\": \"hello\"}");
    assert!(action3.is_some());
    let a3 = action3.unwrap();
    assert_eq!(a3.name, "file_write");

    // 4. Code block
    let action4 = parse_action("Action: bash\n```bash\ncat Cargo.toml\n```");
    assert!(action4.is_some());
    let a4 = action4.unwrap();
    assert_eq!(a4.name, "bash");
    assert_eq!(a4.raw_args, "cat Cargo.toml");

    // 5. Done action
    let action5 = parse_action("Action: done: Here is the final summary of the task.");
    assert!(action5.is_some());
    let a5 = action5.unwrap();
    assert_eq!(a5.name, "done");
    assert_eq!(a5.raw_args, "Here is the final summary of the task.");
}

#[tokio::test]
async fn test_browser_fetch_utf8_arabic() {
    let html_with_arabic = "<html><head><style>body { color: red; }</style></head><body><h1>فيلم إت IT Movie</h1><p>هذا فيلم رعب أمريكي مشهور جداً 🎬</p><script>console.log('secret');</script></body></html>";
    let stripped = clawmind::tools::browser::strip_html_tags(html_with_arabic);
    
    // Ensure scripts and styles are stripped
    assert!(!stripped.contains("color: red"));
    assert!(!stripped.contains("secret"));
    
    // Ensure Arabic and emojis are perfectly preserved without panic or corruption
    assert!(stripped.contains("فيلم إت IT Movie"));
    assert!(stripped.contains("هذا فيلم رعب أمريكي مشهور جداً 🎬"));
}

#[test]
fn test_robust_file_write_parsing() {
    use clawmind::tools::{parse_file_write_args, parse_single_arg_or_json};

    // 1. JSON with trailing <tool_call|> (The exact issue user hit!)
    let input1 = r#"{"path": "/home/ibrahim/Desktop/ClawMind/main.rs", "content": "fn main() {}"}<tool_call|>"#;
    let res1 = parse_file_write_args(input1);
    assert!(res1.is_ok());
    let (p1, c1) = res1.unwrap();
    assert_eq!(p1, "/home/ibrahim/Desktop/ClawMind/main.rs");
    assert_eq!(c1, "fn main() {}");

    // 2. Single quotes in JSON
    let input2 = "{'path': 'src/main.rs', 'content': 'println!(\"hi\");'}";
    let res2 = parse_file_write_args(input2);
    assert!(res2.is_ok());
    let (p2, c2) = res2.unwrap();
    assert_eq!(p2, "src/main.rs");
    assert_eq!(c2, "println!(\"hi\");");

    // 3. Markdown code block
    let input3 = "main.rs\n```rust\nfn main() {\n    println!(\"ok\");\n}\n```";
    let res3 = parse_file_write_args(input3);
    assert!(res3.is_ok());
    let (p3, c3) = res3.unwrap();
    assert_eq!(p3, "main.rs");
    assert!(c3.contains("fn main()"));

    // 4. Critical: Must NEVER create a file named `{"path"`
    let corrupt_json = r#"{"path": "#;
    let res4 = parse_file_write_args(corrupt_json);
    assert!(res4.is_err());

    // 5. parse_single_arg_or_json with <tool_call|>
    let input5 = r#"{"path": "/home/ibrahim/Desktop/ClawMind/main.rs"}<tool_call|>"#;
    let p5 = parse_single_arg_or_json(input5, "path");
    assert_eq!(p5, "/home/ibrahim/Desktop/ClawMind/main.rs");
}

#[test]
fn test_parse_action_parentheses_and_multiline() {
    // Parentheses format (as model often outputs)
    let action1 = parse_action(r#"Action : mouse_click({"x": 500, "y": 500})"#);
    assert!(action1.is_some());
    let a1 = action1.unwrap();
    assert_eq!(a1.name, "mouse_click");
    assert!(a1.raw_args.contains("500"));

    let action2 = parse_action(r#"Action : browser_search("movie named it")"#);
    assert!(action2.is_some());
    let a2 = action2.unwrap();
    assert_eq!(a2.name, "browser_search");
    assert_eq!(a2.raw_args, "\"movie named it\"");

    // Multiline JSON action
    let action3 = parse_action("Action: file_write: {\n  \"path\": \"server.rs\",\n  \"content\": \"use axum;\"\n}");
    assert!(action3.is_some());
    let a3 = action3.unwrap();
    assert_eq!(a3.name, "file_write");
    assert!(a3.raw_args.contains("server.rs"));
    assert!(a3.raw_args.contains("use axum;"));
}

#[test]
fn test_write_file_protects_clawmind_source() {
    let fs = clawmind::tools::FsTool::new();
    let current_dir = std::env::current_dir().unwrap();
    // In ClawMind repo, attempting to write to relative "src/main.rs" must error
    let res = fs.write_file(&current_dir, "src/main.rs", "malicious or accidental content");
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Refusing to overwrite ClawMind's own source code"));
}
