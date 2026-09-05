//! A Language Server, so an editor outside Balaur gets the diagnostics the
//! Balaur editor's Problems dock shows.
//!
//! It speaks LSP on stdin and stdout and calls the same `check_source` the
//! editor calls, so there is one definition of what is wrong with a script.
//!
//! # Not a system in the engine
//!
//! The DAP server ([`balaur_core::dap`]) lives inside a running game because
//! debugging is a conversation with one: a breakpoint has to land at a point
//! in the frame. Checking has no such tie — it needs the script context and
//! nothing else — so this is a process an editor spawns, boots the project
//! once, and then blocks on stdin. No threads, no frame loop, no game.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value as Json};

/// Serve LSP on stdin/stdout until the client says to exit.
///
/// # Errors
/// If the project will not boot, or the streams fail.
fn serve(project_root: &Path) -> Result<()> {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let app = balaur::standard_app(balaur::AppConfig::export(&root))?;
    let host = balaur::rune::rune_of(&app.engine);
    let mut server = Server {
        root,
        host,
        published: BTreeSet::new(),
        open: Vec::new(),
    };
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    while let Some(message) = read_message(&mut reader)? {
        if server.handle(&message, &mut writer)? {
            return Ok(());
        }
    }
    Ok(())
}

/// One `Content-Length`-framed JSON message, or `None` at end of stream.
///
/// The header block is ASCII and ends at a blank line; only `Content-Length`
/// means anything to us, and an unparsable one ends the stream rather than
/// leaving the reader out of step with the frame boundaries.
fn read_message(reader: &mut impl BufRead) -> Result<Option<Json>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Ok(None);
    };
    let mut body = vec![0u8; length];
    std::io::Read::read_exact(reader, &mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

fn write_message(writer: &mut impl Write, message: &Json) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

struct Server {
    root: PathBuf,
    host: balaur::rune::RuneHost,
    /// URIs a `publishDiagnostics` has gone out for, so one that stops having
    /// findings is cleared rather than left showing the last ones.
    published: BTreeSet<String>,
    /// The client's copy of every open file, which is the text to check: an
    /// unsaved buffer is the whole point of asking a language server.
    open: Vec<(String, String)>,
}

impl Server {
    /// Handle one message; `true` means the client asked to exit.
    fn handle(&mut self, message: &Json, writer: &mut impl Write) -> Result<bool> {
        let method = message.get("method").and_then(Json::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        match method {
            "initialize" => {
                let reply = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "capabilities": {
                            // Full text on every change: a script is a few
                            // hundred lines and a check recompiles it whole
                            // anyway, so incremental sync would buy nothing.
                            "textDocumentSync": { "openClose": true, "change": 1, "save": true },
                        },
                        "serverInfo": { "name": "balaur", "version": crate::version::long() },
                    }
                });
                write_message(writer, &reply)?;
            }
            "shutdown" => {
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )?;
            }
            "exit" => return Ok(true),
            "textDocument/didOpen" => {
                let document = &message["params"]["textDocument"];
                if let (Some(uri), Some(text)) =
                    (document["uri"].as_str(), document["text"].as_str())
                {
                    self.set_open(uri, text);
                    self.publish(writer)?;
                }
            }
            "textDocument/didChange" => {
                let uri = message["params"]["textDocument"]["uri"].as_str();
                // Full sync, so the last change carries the whole document.
                let text = message["params"]["contentChanges"]
                    .as_array()
                    .and_then(|changes| changes.last())
                    .and_then(|change| change["text"].as_str());
                if let (Some(uri), Some(text)) = (uri, text) {
                    self.set_open(uri, text);
                    self.publish(writer)?;
                }
            }
            "textDocument/didSave" => self.publish(writer)?,
            "textDocument/didClose" => {
                if let Some(uri) = message["params"]["textDocument"]["uri"].as_str() {
                    self.open.retain(|(open, _)| open != uri);
                    self.publish(writer)?;
                }
            }
            // A request we do not serve still needs an answer, or a client
            // that waits for one hangs.
            _ if id.is_some() => {
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                )?;
            }
            _ => {}
        }
        Ok(false)
    }

    fn set_open(&mut self, uri: &str, text: &str) {
        match self.open.iter_mut().find(|(open, _)| open == uri) {
            Some(entry) => entry.1 = text.to_string(),
            None => self.open.push((uri.to_string(), text.to_string())),
        }
    }

    /// Check every root and send the findings, grouped by the file they are
    /// in. A root is a script a scene attaches; a `mod` submodule is reached
    /// through the root that imports it, and its findings name it.
    fn publish(&mut self, writer: &mut impl Write) -> Result<()> {
        let mut by_file: std::collections::BTreeMap<String, Vec<Json>> =
            std::collections::BTreeMap::new();
        for rel in balaur::scene_scripts(&self.root) {
            let Some(source) = self.source_of(&rel) else {
                continue;
            };
            for one in self.host.check_source(&rel, &source)? {
                by_file
                    .entry(one.file.clone())
                    .or_default()
                    .push(diagnostic(&one));
            }
        }
        // Every file that had findings and no longer does gets an empty list,
        // which is how LSP says "clear what I sent you".
        let now: BTreeSet<String> = by_file.keys().map(|file| self.uri_of(file)).collect();
        for uri in self.published.difference(&now) {
            write_message(writer, &notification(uri, &[]))?;
        }
        for (file, found) in &by_file {
            write_message(writer, &notification(&self.uri_of(file), found))?;
        }
        self.published = now;
        Ok(())
    }

    /// The client's copy of a file if it has one, else what is on disk.
    fn source_of(&self, rel: &str) -> Option<String> {
        let uri = self.uri_of(rel);
        self.open
            .iter()
            .find(|(open, _)| *open == uri)
            .map(|(_, text)| text.clone())
            .or_else(|| std::fs::read_to_string(self.root.join(rel)).ok())
    }

    /// A project-relative path as the `file://` URI a client speaks in. An
    /// absolute one is already what the compiler read it from.
    fn uri_of(&self, file: &str) -> String {
        let path = if Path::new(file).is_absolute() {
            PathBuf::from(file)
        } else {
            self.root.join(file)
        };
        format!("file://{}", path.to_string_lossy())
    }
}

fn notification(uri: &str, diagnostics: &[Json]) -> Json {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics },
    })
}

/// A [`Finding`](balaur::rune::Finding) as an LSP diagnostic. LSP counts
/// lines and characters from zero and Rune counts from one; a finding with no
/// span (line 0) is about the file, and lands on its first line.
fn diagnostic(one: &balaur::rune::Finding) -> Json {
    let start = position(one.line, one.column);
    let end = position(one.end_line.max(one.line), one.end_column.max(one.column));
    json!({
        "range": { "start": start, "end": end },
        "severity": if one.severity == "error" { 1 } else { 2 },
        "source": "balaur",
        "message": one.message,
    })
}

fn position(line: usize, column: usize) -> Json {
    json!({
        "line": line.saturating_sub(1),
        "character": column.saturating_sub(1),
    })
}

/// Boot a project and serve, reporting a failure to boot on stderr: a client
/// that spawned us has nowhere else to read it.
pub(crate) fn run(path: &Path) -> Result<()> {
    serve(path).with_context(|| format!("serving {} over LSP", path.display()))
}
