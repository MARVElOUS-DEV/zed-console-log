use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const URI: &str = "file:///workspace/example.ts";
const MARKER: &str = "/* zed-console-log */";

struct Client {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
    id: u64,
    pending: VecDeque<Value>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Client {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zed-console-log"))
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = value.trim().parse::<usize>().ok();
                    }
                }
                let Some(length) = length else { return };
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                let message =
                    serde_json::from_slice(&body).expect("stdout must contain only LSP frames");
                if sender.send(message).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            input,
            output,
            id: 0,
            pending: VecDeque::new(),
        }
    }

    fn new(settings: Value) -> Self {
        let mut client = Self::spawn();
        let response = client.request(
            "initialize",
            json!({
                "processId": null, "rootUri": null,
                "capabilities": { "workspace": { "workspaceEdit": { "documentChanges": true } } },
                "initializationOptions": settings
            }),
        );
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(
            response["result"]["capabilities"]["positionEncoding"],
            "utf-16"
        );
        assert_eq!(
            response["result"]["capabilities"]["textDocumentSync"]["change"],
            2
        );
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, value: Value) {
        let body = serde_json::to_vec(&value).unwrap();
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.input.write_all(&body).unwrap();
        self.input.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self
                .output
                .recv_timeout(Duration::from_secs(10))
                .expect("server response timed out");
            if message.get("method").is_none() && message["id"] == id {
                return message;
            }
            assert!(
                message.get("id").is_none() || message.get("method").is_some(),
                "unexpected response: {message}"
            );
            self.pending.push_back(message);
        }
    }

    fn open(&mut self, source: &str, language: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": { "uri": URI, "languageId": language, "version": 1, "text": source }
            }),
        );
    }

    fn replace(&mut self, source: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": version },
                "contentChanges": [{ "text": source }]
            }),
        );
    }

    fn actions(&mut self, range: Value, only: &[&str]) -> Vec<Value> {
        let response = self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": URI }, "range": range,
                "context": { "diagnostics": [], "only": only }
            }),
        );
        assert!(response.get("error").is_none(), "{response}");
        response["result"].as_array().unwrap().clone()
    }

    fn finish(mut self) {
        let response = self.request("shutdown", Value::Null);
        assert!(response.get("error").is_none(), "{response}");
        self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                Instant::now() < deadline,
                "server did not exit after shutdown"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn position(source: &str, offset: usize) -> Value {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let character = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    json!({ "line": line, "character": character })
}

fn range(source: &str, needle: &str, select: bool) -> Value {
    let start = source.find(needle).expect("needle missing");
    json!({ "start": position(source, start), "end": position(source, start + if select { needle.len() } else { 0 }) })
}

fn caret() -> Value {
    json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } })
}

fn find<'a>(actions: &'a [Value], title: &str) -> &'a Value {
    actions
        .iter()
        .find(|action| action["title"].as_str().unwrap().starts_with(title))
        .unwrap_or_else(|| panic!("missing action {title}: {actions:?}"))
}

fn byte(source: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap() as usize;
    let character = position["character"].as_u64().unwrap() as usize;
    let mut start = 0;
    for _ in 0..line {
        start += source[start..].find('\n').unwrap() + 1;
    }
    let mut units = 0;
    for (offset, c) in source[start..].char_indices() {
        if units == character {
            return start + offset;
        }
        units += c.len_utf16();
    }
    assert_eq!(units, character);
    source.len()
}

fn apply(source: &str, action: &Value, version: i32) -> String {
    let changes = action["edit"]["documentChanges"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["textDocument"]["uri"], URI);
    assert_eq!(changes[0]["textDocument"]["version"], version);
    apply_edits(source, &changes[0]["edits"])
}

fn apply_edits(source: &str, edits: &Value) -> String {
    let mut edits: Vec<_> = edits
        .as_array()
        .unwrap()
        .iter()
        .map(|edit| {
            (
                byte(source, &edit["range"]["start"]),
                byte(source, &edit["range"]["end"]),
                edit["newText"].as_str().unwrap(),
            )
        })
        .collect();
    edits.sort_by_key(|edit| edit.0);
    for pair in edits.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "overlapping edits");
    }
    let mut result = source.to_owned();
    for (start, end, text) in edits.into_iter().rev() {
        result.replace_range(start..end, text);
    }
    result
}

#[test]
fn insertion_respects_multiline_declarations_and_trailing_comments() {
    let source = "const {\n  value,\n} = make(); // retain this comment\nconsume(value);\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    assert_eq!(actions.len(), 6);
    let updated = apply(source, find(&actions, "Insert console.log"), 1);
    assert!(updated.starts_with(
        "const {\n  value,\n} = make(); // retain this comment\n/* zed-console-log */"
    ));
    assert!(updated.contains("example.ts:4 ~ value:"));
    assert_eq!(updated.matches("make()").count(), 1);
    assert!(updated.ends_with("consume(value);\n"));
    let table = apply(source, find(&actions, "Insert console.table"), 1);
    assert!(table.contains("console.table(value);"));
    client.finish();
}

#[test]
fn parameters_returns_loops_and_catch_stay_in_scope() {
    for (source, needle, expected_before, expected_after) in [
        (
            "function run(value) {\n  'use strict';\n  return value;\n}\n",
            "value",
            "'use strict';",
            "return value;",
        ),
        (
            "function run() {\n  const value = 1;\n  return value;\n}\n",
            "value;\n}",
            "const value = 1;",
            "return value;",
        ),
        (
            "for (const item of items) {\n  consume(item);\n}\n",
            "item",
            "of items) {",
            "consume(item);",
        ),
        (
            "try { fail(); } catch (error) {\n  recover(error);\n}\n",
            "error",
            "catch (error) {",
            "recover(error);",
        ),
        (
            "const run = (value) => {\n  return value;\n};\n",
            "value",
            "=> {",
            "return value;",
        ),
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "typescript");
        let actions = client.actions(range(source, needle, false), &[]);
        let updated = apply(source, find(&actions, "Insert console.log"), 1);
        let log = updated.find(MARKER).unwrap();
        assert!(updated.find(expected_before).unwrap() < log, "{updated}");
        assert!(log < updated.find(expected_after).unwrap(), "{updated}");
        client.finish();
    }
}

#[test]
fn refuses_unsafe_selections_and_unsupported_contexts() {
    for (source, needle, select, language) in [
        ("const x = getValue();\n", "getValue()", true, "typescript"),
        (
            "async function f() { const x = await work(); }\n",
            "await work()",
            true,
            "typescript",
        ),
        ("let x = 0; x++;\n", "x++", true, "typescript"),
        ("let x = 0; x = 1;\n", "x = 1", true, "typescript"),
        (
            "const x = items[index];\n",
            "items[index]",
            true,
            "typescript",
        ),
        (
            "const run = (value) => value;\n",
            "value",
            false,
            "typescript",
        ),
        ("if (ok) consume(value);\n", "value", false, "typescript"),
        (
            "for (const item of items) consume(item);\n",
            "item",
            false,
            "typescript",
        ),
        (
            "import { value } from 'pkg';\n",
            "value",
            false,
            "typescript",
        ),
        ("type Value = string;\n", "Value", false, "typescript"),
        (
            "const view = <Widget value={item} />;\n",
            "item",
            false,
            "typescriptreact",
        ),
        ("class C { value = 1; }\n", "value", false, "typescript"),
        ("value = 1\n", "value", false, "python"),
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, language);
        assert!(
            client
                .actions(range(source, needle, select), &[])
                .is_empty(),
            "{source}"
        );
        client.finish();
    }
}

#[test]
fn js_jsx_ts_and_tsx_are_supported() {
    for (language, source) in [
        ("javascript", "const value = 1;\n"),
        ("javascript", "const view = <Widget />;\n"),
        ("javascriptreact", "const view = <Widget />;\n"),
        ("typescript", "const value: number = 1;\n"),
        ("typescriptreact", "const view: JSX.Element = <Widget />;\n"),
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, language);
        let needle = if source.contains("view") {
            "view"
        } else {
            "value"
        };
        assert!(
            !client.actions(range(source, needle, false), &[]).is_empty(),
            "{language}"
        );
        client.finish();
    }
}

#[test]
fn bulk_operations_preserve_application_logs_and_marker_strings() {
    let source = "const value = 1;\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    let generated = apply(source, find(&actions, "Insert console.log"), 1);
    let source = format!(
        "{generated}console.log('application', value);\nconst text = \"/* zed-console-log */ console.log('fake', value);\";\n// a marker example: /* zed-console-log */ console.log('fake', value);\n"
    );
    client.replace(&source, 2);
    let actions = client.actions(caret(), &["source"]);
    assert!(
        actions
            .iter()
            .all(|a| a["kind"].as_str().unwrap().starts_with("source."))
    );
    let disabled = apply(&source, find(&actions, "Comment generated"), 2);
    assert!(disabled.contains("// /* zed-console-log */ console.log("));
    assert!(disabled.contains("\nconsole.log('application', value);"));
    client.replace(&disabled, 3);
    let actions = client.actions(caret(), &[]);
    let enabled = apply(&disabled, find(&actions, "Uncomment generated"), 3);
    assert_eq!(enabled, source);
    client.replace(&enabled, 4);
    client.notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "prefix": "NEW" } }),
    );
    let actions = client.actions(caret(), &["source.zedConsoleLog.correct"]);
    let corrected = apply(&enabled, find(&actions, "Correct generated"), 4);
    assert!(corrected.contains("'NEW ~ example.ts:2 ~ value:'"));
    client.replace(&corrected, 5);
    let actions = client.actions(caret(), &["source.zedConsoleLog.delete"]);
    let deleted = apply(&corrected, find(&actions, "Delete generated"), 5);
    assert!(!deleted.contains("\n/* zed-console-log */"));
    assert!(deleted.contains("console.log('application', value);"));
    assert!(deleted.contains("\"/* zed-console-log */"));
    client.finish();
}

#[test]
fn selection_bulk_edits_only_intersecting_owned_statements() {
    let source = "const x = 1;\n/* zed-console-log */ console.log('first', x);\n/* zed-console-log */ console.log('second', x);\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(
        range(source, "console.log('first', x)", true),
        &["source.zedConsoleLog.delete"],
    );
    let updated = apply(source, find(&actions, "Delete generated"), 1);
    assert!(!updated.contains("'first'"));
    assert!(updated.contains("'second'"));
    client.finish();
}

#[test]
fn multiline_owned_logs_can_be_disabled_and_restored() {
    let source = "const object = { value: 1 };\n/* zed-console-log */ console.log(\n  'old',\n  object.value\n);\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(caret(), &["source.zedConsoleLog.comment"]);
    let disabled = apply(source, find(&actions, "Comment generated"), 1);
    client.replace(&disabled, 2);
    let actions = client.actions(caret(), &["source.zedConsoleLog.uncomment"]);
    assert_eq!(
        apply(&disabled, find(&actions, "Uncomment generated"), 2),
        source
    );
    client.finish();
}

#[test]
fn preserves_crlf_tabs_and_eof_without_newline() {
    let source = "function run() {\r\n\tconst value = 1;\r\n\tconsume(value);\r\n}";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    let updated = apply(source, find(&actions, "Insert console.log"), 1);
    assert!(updated.contains("\r\n\t/* zed-console-log */"));
    assert!(!updated.replace("\r\n", "").contains('\n'));
    assert!(updated.ends_with('}'));
    client.replace("const value = 1;", 2);
    let actions = client.actions(range("const value = 1;", "value", false), &[]);
    let updated = apply("const value = 1;", find(&actions, "Insert console.log"), 2);
    assert!(updated.contains("example.ts:2"));
    assert!(!updated.ends_with('\n'));
    client.finish();
}

#[test]
fn unsaved_incremental_utf16_changes_and_versions_are_respected() {
    let source = "const emoji = '😀'; const café = 1;\nconsume(café);\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let change = range(source, "1", true);
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "range": change, "text": "2" }]
        }),
    );
    let source = source.replace("= 1", "= 2");
    let actions = client.actions(range(&source, "café", false), &[]);
    let updated = apply(&source, find(&actions, "Insert console.log"), 2);
    assert!(updated.contains("const café = 2;"));
    assert!(updated.contains(", café);"));
    client.replace("broken", 1); // stale notification must be ignored
    let actions = client.actions(range(&source, "café", false), &[]);
    apply(&source, find(&actions, "Insert console.log"), 2);
    client.finish();
}

#[test]
fn invalid_changes_disable_actions_until_reopened() {
    let mut client = Client::new(Value::Null);
    client.open("const x = '😀';\n", "typescript");
    client.notify("textDocument/didChange", json!({
        "textDocument": { "uri": URI, "version": 2 },
        "contentChanges": [{
            "range": { "start": { "line": 0, "character": 12 }, "end": { "line": 0, "character": 12 } },
            "text": "broken"
        }]
    }));
    assert!(client.actions(caret(), &[]).is_empty());
    client.open("const x = 1;\n", "typescript");
    assert!(
        !client
            .actions(range("const x = 1;\n", "x", false), &[])
            .is_empty()
    );
    client.notify(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": URI } }),
    );
    assert!(client.actions(caret(), &[]).is_empty());
    client.finish();
}

#[test]
fn validates_configuration_and_escapes_labels() {
    for settings in [
        json!({ "customFunction": "logger.log); evil(" }),
        json!({ "customFunction": "return.log" }),
        json!({ "unknown": true }),
        json!({ "indentSize": 0 }),
        json!({ "prefix": "two\nlines" }),
        json!({ "methods": ["trace"] }),
    ] {
        let mut client = Client::spawn();
        let response = client.request(
            "initialize",
            json!({
                "processId": null, "capabilities": {}, "initializationOptions": settings
            }),
        );
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    let source = "const value = 1;\n";
    let mut client = Client::new(json!({
        "prefix": "a'b\\c", "methods": ["warn", "warn"], "customFunction": "logger.debug", "quote": "single"
    }));
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    assert_eq!(actions.len(), 2);
    let updated = apply(source, find(&actions, "Insert logger.debug"), 1);
    assert!(updated.contains("a\\'b\\\\c"));
    client.notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "customFunction": "bad()" } }),
    );
    let actions = client.actions(range(source, "value", false), &[]);
    assert_eq!(
        actions.len(),
        2,
        "invalid configuration must retain previous settings"
    );
    client.finish();
}

#[test]
fn malformed_buffers_and_invalid_ranges_do_not_crash_server() {
    let mut client = Client::new(Value::Null);
    client.open("const x = ;\n", "typescript");
    assert!(client.actions(caret(), &[]).is_empty());
    client.replace("const x = 1;\n", 2);
    let response = client.request("textDocument/codeAction", json!({
        "textDocument": { "uri": URI },
        "range": { "start": { "line": 99, "character": 0 }, "end": { "line": 99, "character": 0 } },
        "context": { "diagnostics": [] }
    }));
    assert_eq!(response["error"]["code"], -32602);
    assert!(
        !client
            .actions(range("const x = 1;\n", "x", false), &[])
            .is_empty()
    );
    client.finish();
}

#[test]
fn conditional_and_default_references_are_not_hoisted() {
    for (source, needle) in [
        ("const value = false ? missing : 1;\n", "missing"),
        ("const value = false && missing;\n", "missing"),
        (
            "function run(value = fallback) {\n  return value;\n}\n",
            "fallback",
        ),
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "typescript");
        assert!(
            client.actions(range(source, needle, false), &[]).is_empty(),
            "{source}"
        );
        client.finish();
    }
}

#[test]
fn inline_bulk_comments_cannot_swallow_application_statements() {
    let source = "const x = 1; /* zed-console-log */ console.log('old', x); application();\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    assert!(
        client
            .actions(caret(), &["source.zedConsoleLog.comment"])
            .is_empty()
    );
    let actions = client.actions(caret(), &["source.zedConsoleLog.delete"]);
    let updated = apply(source, find(&actions, "Delete generated"), 1);
    assert!(updated.contains("application();"));
    client.finish();
}

#[test]
fn disabled_logs_in_unbraced_control_flow_are_not_enabled() {
    let source = "const x = 1;\nif (x)\n  // /* zed-console-log */ console.log('old', x);\n  application();\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    assert!(
        client
            .actions(caret(), &["source.zedConsoleLog.uncomment"])
            .is_empty()
    );
    client.finish();
}

#[test]
fn semicolon_preferences_keep_asi_boundaries_and_method_actions_are_unique() {
    let source = "const value = 1;\n(value).toString();\n";
    let mut client = Client::new(json!({ "semicolon": false, "customFunction": "console.log" }));
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    assert_eq!(actions.len(), 6);
    let updated = apply(source, find(&actions, "Insert console.log"), 1);
    assert!(updated.contains(", value);\n(value).toString();"));
    client.replace("const value = 1;\n", 2);
    let actions = client.actions(range("const value = 1;\n", "value", false), &[]);
    let updated = apply(
        "const value = 1;\n",
        find(&actions, "Insert console.log"),
        2,
    );
    assert!(updated.ends_with(", value)\n"));
    client.finish();
}

#[test]
fn multiline_initializers_use_statement_indentation() {
    let source = "function run() {\n  const value =\n      work();\n  return value;\n}\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(range(source, "value", false), &[]);
    let updated = apply(source, find(&actions, "Insert console.log"), 1);
    assert!(updated.contains("\n  /* zed-console-log */"));
    assert!(updated.contains("example.ts:4"));
    client.finish();
}

#[test]
fn correction_preserves_multiline_formatting_and_final_line_numbers() {
    let source = "const x = 1;\n/* zed-console-log */\nconsole.log(\n  'old',\n  x\n)\n/* zed-console-log */ console.log('other', x);\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "typescript");
    let actions = client.actions(caret(), &["source.zedConsoleLog.correct"]);
    let updated = apply(source, find(&actions, "Correct generated"), 1);
    assert_eq!(updated.lines().count(), source.lines().count());
    assert!(updated.contains("example.ts:3 ~ x:"));
    assert!(updated.contains("example.ts:7 ~ x:"));
    assert!(updated.contains("\nconsole.log(\n  "));
    client.finish();
}

#[test]
fn bulk_edits_withhold_statement_merges_even_when_the_result_parses() {
    for newline in ["\n", "\r\n"] {
        for continuation in ["[1].forEach(run)", "(run)()", "`template`"] {
            for logs in [
                "/* zed-console-log */ console.log('owned', value);\n",
                "/* zed-console-log */ console.log('first', value);\n/* zed-console-log */ console.log('second', value);\n",
            ] {
                let source =
                    format!("console.log('application')\n{logs}// retain\n{continuation}\n")
                        .replace('\n', newline);
                let mut client = Client::new(Value::Null);
                client.open(&source, "javascript");
                for operation in ["comment", "delete"] {
                    let kind = format!("source.zedConsoleLog.{operation}");
                    assert!(
                        client.actions(caret(), &[&kind]).is_empty(),
                        "{operation}: {source}"
                    );
                }
                client.finish();
            }
        }
    }
    // Enabling a log must not absorb the following application statement.
    let source = "const value = 1;\n// /* zed-console-log */ console.log('owned', value)\n[1].forEach(run)\n";
    let mut client = Client::new(Value::Null);
    client.open(source, "javascript");
    assert!(
        client
            .actions(caret(), &["source.zedConsoleLog.uncomment"])
            .is_empty()
    );
    client.finish();
}

#[test]
fn bulk_edits_keep_safe_explicit_boundaries_available() {
    for (prefix, suffix) in [
        ("console.log('application');", "[1].forEach(run)"),
        ("console.log('application');", "(run)()"),
        ("console.log('application')", "run()"),
    ] {
        let source =
            format!("{prefix}\n/* zed-console-log */ console.log('owned', value);\n{suffix}\n");
        let mut client = Client::new(Value::Null);
        client.open(&source, "javascript");
        for operation in ["Comment", "Delete"] {
            let actions = client.actions(caret(), &["source"]);
            let updated = apply(
                &source,
                find(&actions, &format!("{operation} generated")),
                1,
            );
            assert!(updated.starts_with(&format!("{prefix}\n")), "{updated}");
            assert!(updated.ends_with(&format!("{suffix}\n")), "{updated}");
        }
        client.finish();
    }
}

#[test]
fn loop_header_logs_require_the_same_binding_in_the_body() {
    for source in [
        "for (const item of items) { const item = 1; consume(item); }",
        "for (const item in items) { let item; consume(item); }",
        "for (let item = 0; item < 1; item++) { class item {} }",
        "for (const { item } of items) { const item = 1; }",
        "for (const item of items) { consume(); const item = 1; }",
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "javascript");
        for select in [false, true] {
            assert!(
                client
                    .actions(range(source, "item", select), &[])
                    .is_empty(),
                "{source}"
            );
        }
        client.finish();
    }
    for source in [
        "for (const item of items) { { const item = 1; consume(item); } }",
        "for (var item in items) { var item; consume(item); }",
        "for (let item = 0; item < 1; item++) {}",
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "javascript");
        let actions = client.actions(range(source, "item", false), &[]);
        apply(source, find(&actions, "Insert console.log"), 1);
        client.finish();
    }
}

#[test]
fn optional_chain_descendants_are_not_read_unconditionally() {
    for source in [
        "const obj = null; const x = obj?.method(missing);",
        "const obj = null; const x = obj?.[missing];",
        "const obj = null; const x = obj.method?.(missing);",
        "const obj = null; const x = obj?.method(other(missing));",
        "const obj = null; const x = obj?.method()[missing];",
        "const obj = null; const x = obj?.[missing.value];",
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "javascript");
        for select in [false, true] {
            assert!(
                client
                    .actions(range(source, "missing", select), &[])
                    .is_empty(),
                "{source}"
            );
        }
        client.finish();
    }
}

#[test]
fn typeof_operands_require_a_resolved_identifier() {
    for (source, needle) in [
        ("const x = typeof missing;", "missing"),
        ("const x = typeof (missing);", "(missing)"),
        (
            "const x = typeof (missing as unknown);",
            "missing as unknown",
        ),
    ] {
        let mut client = Client::new(Value::Null);
        client.open(source, "typescript");
        for select in [false, true] {
            assert!(
                client
                    .actions(range(source, needle, select), &[])
                    .is_empty(),
                "{source}"
            );
        }
        client.finish();
    }
    let source = "const known = 1; const x = typeof known;";
    let mut client = Client::new(Value::Null);
    client.open(source, "javascript");
    let start = source.rfind("known").unwrap();
    let selection = json!({ "start": position(source, start), "end": position(source, start + 5) });
    let actions = client.actions(selection, &[]);
    apply(source, find(&actions, "Insert console.log"), 1);
    client.finish();
}

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "zed-console-log-{}-{unique}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn uri(&self, relative: &str) -> String {
        url::Url::from_file_path(self.0.join(relative))
            .unwrap()
            .to_string()
    }

    fn write(&self, relative: &str, source: &str) -> String {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        self.uri(relative)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

impl Client {
    fn workspace(roots: &[&Path]) -> Self {
        Self::workspace_options(json!({
            "workspaceFolders": roots.iter().map(|root| json!({
                "uri": url::Url::from_file_path(root).unwrap().to_string(), "name": "test"
            })).collect::<Vec<_>>(),
            "capabilities": { "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } } }
        }))
    }

    fn workspace_options(options: Value) -> Self {
        let mut client = Self::spawn();
        let response = client.request("initialize", options);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(
            response["result"]["capabilities"]["executeCommandProvider"]["commands"],
            json!(["zedConsoleLog.deleteWorkspace"])
        );
        assert_eq!(
            response["result"]["capabilities"]["workspace"]["workspaceFolders"]["supported"],
            true
        );
        client.notify("initialized", json!({}));
        client
    }

    fn open_uri(&mut self, uri: &str, source: &str, language: &str, version: i32) {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": {
                "uri": uri, "languageId": language, "version": version, "text": source
            }}),
        );
    }

    fn workspace_actions(&mut self, uri: &str, range: Value, only: &[&str]) -> Vec<Value> {
        self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri }, "range": range,
                "context": { "diagnostics": [], "only": only }
            }),
        )["result"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn begin_cleanup(&mut self) {
        let response = self.request(
            "workspace/executeCommand",
            json!({ "command": "zedConsoleLog.deleteWorkspace" }),
        );
        assert!(response.get("error").is_none(), "{response}");
    }

    fn wait_message(&mut self, method: &str) -> Value {
        loop {
            let message = self.pending.pop_front().unwrap_or_else(|| {
                self.output
                    .recv_timeout(Duration::from_secs(10))
                    .expect("server message timed out")
            });
            if message["method"] == method {
                return message;
            }
            assert!(
                message.get("id").is_none(),
                "unexpected request while waiting for {method}: {message}"
            );
        }
    }

    fn respond(&mut self, request: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": result }));
    }
}

const OWNED: &str = "const value = 1;\n/* zed-console-log */ console.log('owned', value);\n";

#[test]
fn workspace_cleanup_previews_counts_and_edits_unsaved_and_closed_files() {
    let workspace = Workspace::new();
    let open_uri = workspace.write("open café.ts", OWNED);
    let unsaved = "const café = '😀';\r\n/* zed-console-log */ console.log('unsaved', café);\r\nconsole.log('application', café);\r\n";
    let closed = "const value = 1;\n/* zed-console-log */ console.log(\n  'multiline',\n  value\n);\n// /* zed-console-log */ console.log('disabled', value);\nconsole.log('application', value);\nconst text = \"/* zed-console-log */ console.log('fake', value);\";\n";
    let closed_uri = workspace.write("nested/closed.tsx", closed);
    let clean_uri = workspace.write("clean.ts", "console.log('application');\n");
    let mut client = Client::workspace(&[&workspace.0]);
    client.open_uri(&open_uri, unsaved, "typescript", 7);
    let actions = client.workspace_actions(
        &open_uri,
        caret(),
        &["source.zedConsoleLog.deleteWorkspace"],
    );
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["title"], "Delete generated logs in workspace…");
    assert_eq!(
        actions[0]["command"]["command"],
        "zedConsoleLog.deleteWorkspace"
    );
    assert!(actions[0].get("edit").is_none());
    client.begin_cleanup();
    let preview = client.wait_message("window/showMessageRequest");
    let message = preview["params"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("Delete 3 generated logs in 2 files?"),
        "{message}"
    );
    assert!(message.contains("open café.ts — 1 log"));
    assert!(message.contains("closed.tsx — 2 logs"));
    assert!(!message.contains("clean.ts"));
    assert_eq!(
        fs::read_to_string(workspace.0.join("open café.ts")).unwrap(),
        OWNED
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("nested/closed.tsx")).unwrap(),
        closed
    );
    client.respond(&preview, json!({ "title": "Delete logs" }));
    let request = client.wait_message("workspace/applyEdit");
    let changes = request["params"]["edit"]["documentChanges"]
        .as_array()
        .unwrap();
    assert_eq!(changes.len(), 2);
    for change in changes {
        if change["textDocument"]["uri"] == open_uri {
            assert_eq!(change["textDocument"]["version"], 7);
            assert_eq!(
                apply_edits(unsaved, &change["edits"]),
                "const café = '😀';\r\nconsole.log('application', café);\r\n"
            );
        } else {
            assert_eq!(change["textDocument"]["uri"], closed_uri);
            assert!(change["textDocument"]["version"].is_null());
            let updated = apply_edits(closed, &change["edits"]);
            assert!(!updated.contains("'multiline'"));
            assert!(!updated.contains("'disabled'"));
            assert!(updated.contains("console.log('application', value);"));
            assert!(updated.contains("'fake'"));
        }
        assert_ne!(change["textDocument"]["uri"], clean_uri);
    }
    client.respond(&request, json!({ "applied": true }));
    let result = client.wait_message("window/showMessage");
    assert!(
        result["params"]["message"]
            .as_str()
            .unwrap()
            .contains("Removed 3 generated logs from 2 files")
    );
    // The server never writes directly, even when the client accepts the edits.
    assert_eq!(
        fs::read_to_string(workspace.0.join("open café.ts")).unwrap(),
        OWNED
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("nested/closed.tsx")).unwrap(),
        closed
    );
    client.finish();
}

#[test]
fn workspace_cleanup_respects_ignore_rules_generated_directories_and_safety_checks() {
    let workspace = Workspace::new();
    workspace.write(".gitignore", "ignored/\n*.generated.ts\n");
    workspace.write(".ignore", "local-only/\n");
    workspace.write("nested/.gitignore", "skip.ts\n");
    for relative in [
        "ignored/file.ts",
        "local-only/file.ts",
        "a.generated.ts",
        "nested/skip.ts",
        "node_modules/pkg/file.js",
        "dist/file.js",
        "target/file.ts",
        "build/file.ts",
        "coverage/file.ts",
        ".next/file.ts",
        "vendor/file.ts",
        "not-code.txt",
    ] {
        workspace.write(relative, OWNED);
    }
    workspace.write(
        "broken.ts",
        "const value = ;\n/* zed-console-log */ console.log('owned', value);\n",
    );
    workspace.write("asi.js", "console.log('application')\n/* zed-console-log */ console.log('owned', value);\n[1].forEach(run)\n");
    workspace.write("oversized.ts", &" ".repeat(4 * 1024 * 1024 + 1));
    let valid = workspace.write(".hidden/valid.mts", OWNED);
    let mut client = Client::workspace(&[&workspace.0]);
    // Ignored open buffers must not bypass ignore rules.
    client.open_uri(&workspace.uri("ignored/file.ts"), OWNED, "typescript", 1);
    client.begin_cleanup();
    let preview = client.wait_message("window/showMessageRequest");
    let message = preview["params"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("Delete 1 generated log in 1 file?"),
        "{message}"
    );
    assert!(message.contains("3 files/entries skipped"), "{message}");
    assert!(!message.contains("ignored/file.ts"));
    client.respond(&preview, json!({ "title": "Delete logs" }));
    let apply = client.wait_message("workspace/applyEdit");
    assert_eq!(
        apply["params"]["edit"]["documentChanges"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        apply["params"]["edit"]["documentChanges"][0]["textDocument"]["uri"],
        valid
    );
    client.respond(&apply, json!({ "applied": true }));
    client.wait_message("window/showMessage");
    client.finish();
}

#[test]
fn workspace_cleanup_cancel_or_dismiss_never_requests_edits() {
    for choice in [Value::Null, json!({ "title": "Cancel" })] {
        let workspace = Workspace::new();
        workspace.write("value.ts", OWNED);
        let mut client = Client::workspace(&[&workspace.0]);
        client.begin_cleanup();
        let preview = client.wait_message("window/showMessageRequest");
        // The ordered protocol loop remains responsive while the prompt is open.
        let duplicate = client.request(
            "workspace/executeCommand",
            json!({ "command": "zedConsoleLog.deleteWorkspace" }),
        );
        assert_eq!(duplicate["error"]["code"], -32602);
        client.respond(&preview, choice);
        let result = client.wait_message("window/showMessage");
        assert_eq!(result["params"]["message"], "Workspace cleanup canceled.");
        assert_eq!(
            fs::read_to_string(workspace.0.join("value.ts")).unwrap(),
            OWNED
        );
        client.finish();
    }
}

#[test]
fn workspace_cleanup_rejects_changed_previews_instead_of_applying_stale_edits() {
    for change in ["disk", "buffer", "folder", "open"] {
        let workspace = Workspace::new();
        let uri = workspace.write("value.ts", OWNED);
        let mut client = Client::workspace(&[&workspace.0]);
        if change == "buffer" {
            client.open_uri(&uri, OWNED, "typescript", 1);
        }
        client.begin_cleanup();
        let preview = client.wait_message("window/showMessageRequest");
        match change {
            "disk" => { workspace.write("value.ts", "const value = 2;\n"); }
            "buffer" => client.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": "const value = 2;\n" }] })),
            "folder" => client.notify("workspace/didChangeWorkspaceFolders", json!({ "event": { "added": [], "removed": [{ "uri": workspace.uri(""), "name": "test" }] } })),
            "open" => client.open_uri(&uri, "const value = 2;\n", "typescript", 1),
            _ => unreachable!(),
        }
        // Reply immediately: queued notifications must be drained before validation.
        client.respond(&preview, json!({ "title": "Delete logs" }));
        let result = client.wait_message("window/showMessage");
        assert!(
            result["params"]["message"]
                .as_str()
                .unwrap()
                .contains("changed"),
            "{result}"
        );
        client.finish();
    }
}

#[test]
fn workspace_cleanup_never_falls_back_to_disk_for_invalid_open_buffers() {
    let workspace = Workspace::new();
    let uri = workspace.write("value.ts", OWNED);
    let mut client = Client::workspace(&[&workspace.0]);
    client.open_uri(&uri, OWNED, "typescript", 1);
    client.notify("textDocument/didChange", json!({
        "textDocument": { "uri": uri, "version": 2 },
        "contentChanges": [{ "range": { "start": { "line": 500, "character": 0 }, "end": { "line": 500, "character": 1 } }, "text": "invalid" }]
    }));
    client.begin_cleanup();
    let result = client.wait_message("window/showMessage");
    assert!(
        result["params"]["message"]
            .as_str()
            .unwrap()
            .contains("No safely removable generated logs found. 1 files/entries skipped.")
    );
    client.finish();
}

#[test]
fn workspace_cleanup_tracks_multiple_folders_and_deduplicates_nested_roots() {
    let first = Workspace::new();
    let second = Workspace::new();
    let outside = Workspace::new();
    first.write("value.ts", OWNED);
    first.write("nested/value.ts", OWNED);
    second.write("value.cts", OWNED);
    let outside_uri = outside.write("value.ts", OWNED);
    let nested = first.0.join("nested");
    let mut client = Client::workspace(&[&first.0, &nested, &outside.0]);
    client.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({ "event": {
            "added": [{ "uri": second.uri(""), "name": "second" }],
            "removed": [{ "uri": outside.uri(""), "name": "outside" }]
        }}),
    );
    client.open_uri(&outside_uri, OWNED, "typescript", 1);
    assert!(
        client
            .workspace_actions(
                &outside_uri,
                caret(),
                &["source.zedConsoleLog.deleteWorkspace"]
            )
            .is_empty()
    );
    client.begin_cleanup();
    let preview = client.wait_message("window/showMessageRequest");
    assert!(
        preview["params"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Delete 3 generated logs in 3 files?")
    );
    client.respond(&preview, json!({ "title": "Cancel" }));
    client.wait_message("window/showMessage");
    client.finish();
}

#[test]
fn workspace_cleanup_requires_client_capabilities_and_never_replaces_document_cleanup() {
    let workspace = Workspace::new();
    let uri = workspace.write("value.ts", OWNED);
    let mut client = Client::workspace_options(json!({
        "rootUri": workspace.uri(""),
        "capabilities": { "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } } }
    }));
    client.open_uri(&uri, OWNED, "typescript", 1);
    assert_eq!(
        client
            .workspace_actions(&uri, caret(), &["source.zedConsoleLog.deleteWorkspace"])
            .len(),
        1
    );
    let document = client.workspace_actions(&uri, caret(), &["source.zedConsoleLog.delete"]);
    assert_eq!(document.len(), 1);
    assert!(document[0].get("edit").is_some());
    assert!(
        client
            .workspace_actions(
                &uri,
                range(OWNED, "value", true),
                &["source.zedConsoleLog.deleteWorkspace"]
            )
            .is_empty()
    );
    let unknown = client.request(
        "workspace/executeCommand",
        json!({ "command": "untrusted", "arguments": ["/outside"] }),
    );
    assert_eq!(unknown["error"]["code"], -32602);
    client.finish();
    for workspace_options in [
        json!({}),
        json!({ "applyEdit": true }),
        json!({ "workspaceEdit": { "documentChanges": true } }),
    ] {
        let mut client = Client::workspace_options(
            json!({ "rootUri": workspace.uri(""), "capabilities": { "workspace": workspace_options } }),
        );
        client.open_uri(&uri, OWNED, "typescript", 1);
        assert!(
            client
                .workspace_actions(&uri, caret(), &["source.zedConsoleLog.deleteWorkspace"])
                .is_empty()
        );
        let response = client.request(
            "workspace/executeCommand",
            json!({ "command": "zedConsoleLog.deleteWorkspace" }),
        );
        assert_eq!(response["error"]["code"], -32602);
        client.finish();
    }
}

#[test]
fn workspace_cleanup_reports_editor_rejection_without_writing_files() {
    let workspace = Workspace::new();
    workspace.write("value.ts", OWNED);
    let mut client = Client::workspace(&[&workspace.0]);
    client.begin_cleanup();
    let preview = client.wait_message("window/showMessageRequest");
    client.respond(&preview, json!({ "title": "Delete logs" }));
    let apply = client.wait_message("workspace/applyEdit");
    client.respond(
        &apply,
        json!({ "applied": false, "failureReason": "version mismatch" }),
    );
    let result = client.wait_message("window/showMessage");
    assert!(
        result["params"]["message"]
            .as_str()
            .unwrap()
            .contains("version mismatch")
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("value.ts")).unwrap(),
        OWNED
    );
    client.finish();
}

#[cfg(unix)]
#[test]
fn workspace_cleanup_does_not_follow_symlinks() {
    let workspace = Workspace::new();
    let outside = Workspace::new();
    outside.write("outside.ts", OWNED);
    std::os::unix::fs::symlink(outside.0.join("outside.ts"), workspace.0.join("link.ts")).unwrap();
    std::os::unix::fs::symlink(&outside.0, workspace.0.join("linked-dir")).unwrap();
    let uri = workspace.write("valid.ts", OWNED);
    workspace.write("aliased.ts", OWNED);
    std::os::unix::fs::symlink(
        workspace.0.join("aliased.ts"),
        workspace.0.join("internal-link.ts"),
    )
    .unwrap();
    let mut client = Client::workspace(&[&workspace.0]);
    client.open_uri(&workspace.uri("internal-link.ts"), OWNED, "typescript", 1);
    client.open_uri(&workspace.uri("link.ts"), OWNED, "typescript", 1);
    client.begin_cleanup();
    let preview = client.wait_message("window/showMessageRequest");
    assert!(
        preview["params"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Delete 1 generated log in 1 file?")
    );
    client.respond(&preview, json!({ "title": "Delete logs" }));
    let apply = client.wait_message("workspace/applyEdit");
    assert_eq!(
        apply["params"]["edit"]["documentChanges"][0]["textDocument"]["uri"],
        uri
    );
    assert_eq!(
        apply["params"]["edit"]["documentChanges"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    client.respond(&apply, json!({ "applied": true }));
    client.wait_message("window/showMessage");
    client.finish();
}

#[test]
fn workspace_cleanup_rejects_missing_or_non_file_workspace_roots() {
    let workspace = Workspace::new();
    let uri = workspace.write("value.ts", OWNED);
    // Non-file schemes must never be interpreted as filesystem paths.
    for root in [
        Value::Null,
        json!(format!("https://example.com{}", workspace.0.display())),
    ] {
        let mut client = Client::workspace_options(json!({
            "rootUri": root,
            "capabilities": { "workspace": { "applyEdit": true, "workspaceEdit": { "documentChanges": true } } }
        }));
        client.open_uri(&uri, OWNED, "typescript", 1);
        assert!(
            client
                .workspace_actions(&uri, caret(), &["source.zedConsoleLog.deleteWorkspace"])
                .is_empty()
        );
        let response = client.request(
            "workspace/executeCommand",
            json!({ "command": "zedConsoleLog.deleteWorkspace" }),
        );
        assert_eq!(response["error"]["code"], -32602);
        client.finish();
    }
}
