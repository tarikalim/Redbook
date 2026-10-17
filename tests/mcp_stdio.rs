//! Pipes JSON-RPC lines into `redbook mcp` with a temp database and checks the replies.

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_over_stdio() {
    let dir = std::env::temp_dir().join(format!("redbook-mcp-{}", std::process::id()));
    let req = |id: u64, method: &str, params: Value| {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    };
    let call = |id, name: &str, args: Value| {
        req(id, "tools/call", json!({ "name": name, "arguments": args }))
    };
    let lines = [
        req(
            1,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }),
        ),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
        req(2, "tools/list", json!({})),
        call(
            3,
            "create_page",
            json!({ "title": "Stdio page", "markdown": "## Hello\n\nquokka facts" }),
        ),
        call(4, "search", json!({ "query": "quokka" })),
        req(5, "ping", json!({})),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_redbook"))
        .arg("mcp")
        .env("REDBOOK_DB", dir.join("test.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all((lines.join("\n") + "\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 5, "{replies:?}"); // no reply to the notification
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
    assert!(replies[1]["result"]["tools"].as_array().unwrap().len() > 10);
    let text = |r: &Value| {
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let created = text(&replies[2]);
    let id = &created[created.find("id: ").unwrap() + 4..][..36];
    assert!(text(&replies[3]).contains(id), "{replies:?}");
    assert_eq!(replies[4]["result"], json!({}));

    // get_page in a second session sees the committed page.
    let mut child = Command::new(env!("CARGO_BIN_EXE_redbook"))
        .arg("mcp")
        .env("REDBOOK_DB", dir.join("test.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let line = call(1, "get_page", json!({ "page_id": id })) + "\n";
    child
        .stdin
        .take()
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let reply: Value = serde_json::from_str(out.trim()).unwrap();
    let page = text(&reply);
    assert!(
        page.contains("# Stdio page") && page.contains("## Hello"),
        "{page}"
    );
    std::fs::remove_dir_all(dir).ok();
}
