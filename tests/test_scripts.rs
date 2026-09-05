use clawmind::tools::scripts::ScriptsTool;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn create_test_dir() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("clawmind_script_test_{}_{}", std::process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_scripts_scan_and_metadata_parsing() {
    let scripts_path = create_test_dir();

    // Create a sample python script with docstring
    let py_content = r#"#!/usr/bin/env python3
"""
Description: Calculates sales metrics from CSV reports
Usage: sales_report.py --input <path> --output <path>
"""
import sys
print("Sales calculation complete")
"#;
    fs::write(scripts_path.join("sales_report.py"), py_content).unwrap();

    // Create a sample bash script with comments
    let sh_content = r#"#!/bin/bash
# Description: Synchronizes backups to S3 storage bucket
# Usage: sync_s3.sh <bucket_name>
echo "Backup synced to $1"
"#;
    fs::write(scripts_path.join("sync_s3.sh"), sh_content).unwrap();

    let tool = ScriptsTool::with_dir(&scripts_path);
    let catalog = tool.scan_catalog();

    assert_eq!(catalog.len(), 2);

    let py_info = catalog.iter().find(|s| s.name == "sales_report.py").unwrap();
    assert_eq!(py_info.interpreter, "python3");
    assert!(py_info.description.contains("Calculates sales metrics"));
    assert!(py_info.usage.contains("sales_report.py --input"));

    let sh_info = catalog.iter().find(|s| s.name == "sync_s3.sh").unwrap();
    assert_eq!(sh_info.interpreter, "bash");
    assert!(sh_info.description.contains("Synchronizes backups"));
    assert!(sh_info.usage.contains("sync_s3.sh <bucket_name>"));

    // Verify RAG prompt formatting
    let rag_prompt = tool.format_rag_prompt();
    assert!(rag_prompt.contains("AUTOMATION SCRIPT ARSENAL (Script-RAG)"));
    assert!(rag_prompt.contains("sales_report.py"));
    assert!(rag_prompt.contains("sync_s3.sh"));
}

#[tokio::test]
async fn test_scripts_execution_and_creation() {
    let scripts_path = create_test_dir();
    let tool = ScriptsTool::with_dir(&scripts_path);

    // Create script via tool
    let script_code = r#"#!/usr/bin/env bash
# Description: Prints greeting for testing
# Usage: greet.sh <name>
echo "Hello, $1!"
"#;
    let create_res = tool.create_script("greet.sh", script_code);
    assert!(create_res.is_ok());

    // Inspect script
    let inspect_res = tool.inspect_script("greet.sh");
    assert!(inspect_res.is_ok());
    let inspect_txt = inspect_res.unwrap();
    assert!(inspect_txt.contains("Prints greeting for testing"));

    // Execute script
    let exec_res = tool.execute_script("greet.sh", "ClawMind").await;
    assert!(exec_res.is_ok());
    let output = exec_res.unwrap();
    assert!(output.contains("Hello, ClawMind!"));
    assert!(output.contains("🟢 Success"));
}

#[tokio::test]
async fn test_scripts_missing_and_invalid() {
    let scripts_path = create_test_dir();
    let tool = ScriptsTool::with_dir(&scripts_path);

    // Missing script execution
    let err_run = tool.execute_script("non_existent.sh", "").await;
    assert!(err_run.is_err());

    // Missing script inspection
    let err_inspect = tool.inspect_script("non_existent.py");
    assert!(err_inspect.is_err());

    // Invalid script name creation
    let err_create = tool.create_script("../hack.sh", "echo 1");
    assert!(err_create.is_err());
}
