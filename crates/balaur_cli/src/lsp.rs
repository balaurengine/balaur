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
use serde_json::{Value as Json, json};

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
                            // `:` and `.` are the two characters that change
                            // what may follow; the rest arrive on a keystroke.
                            "completionProvider": { "triggerCharacters": [".", ":"] },
                            "hoverProvider": true,
                            "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
                            "documentFormattingProvider": true,
                            "definitionProvider": true,
                            "documentSymbolProvider": true,
                            "referencesProvider": true,
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
            "textDocument/completion" => {
                let items = self.at(&message["params"], |host, key, source, line, column| {
                    Ok(host
                        .complete(key, source, line, column)?
                        .iter()
                        .map(completion)
                        .collect())
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": items }),
                )?;
            }
            "textDocument/hover" => {
                let found = self.one(&message["params"], |host, key, source, line, column| {
                    Ok(host.hover(key, source, line, column)?.map(|h| {
                        json!({ "contents": { "kind": "markdown", "value": markdown(&h) } })
                    }))
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": found }),
                )?;
            }
            "textDocument/signatureHelp" => {
                let found = self.one(&message["params"], |host, key, source, line, column| {
                    Ok(host
                        .signature_help(key, source, line, column)?
                        .map(|(h, active)| {
                            json!({
                                "signatures": [{
                                    "label": format!("{}{}", h.title, h.detail),
                                    "documentation": h.doc,
                                }],
                                "activeSignature": 0,
                                "activeParameter": active,
                            })
                        }))
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": found }),
                )?;
            }
            "textDocument/formatting" => {
                let edit = self.formatting(&message["params"]);
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": edit }),
                )?;
            }
            "textDocument/definition" => {
                let found = self.one(&message["params"], |host, key, source, line, column| {
                    // A definition with no file is engine API; the URL is
                    // what a client should open, so it goes back as one.
                    Ok(host.definition(key, source, line, column)?.map(|d| {
                        if d.file.is_empty() {
                            json!({ "uri": d.url, "range": span(1, 1) })
                        } else {
                            json!({ "uri": self.uri_of(&d.file), "range": span(d.line, d.column) })
                        }
                    }))
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": found }),
                )?;
            }
            "textDocument/documentSymbol" => {
                let items = self.whole(&message["params"], |host, key, source| {
                    Ok(host
                        .symbols(key, source)?
                        .iter()
                        .map(|one| {
                            json!({
                                "name": one.name,
                                "kind": if one.kind == balaur::rune::Kind::Function { 12 } else { 7 },
                                "detail": one.detail,
                                "range": span(one.line.max(1), one.column),
                                "selectionRange": span(one.line.max(1), one.column),
                            })
                        })
                        .collect())
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": items }),
                )?;
            }
            "textDocument/references" => {
                let items = self.at(&message["params"], |host, key, source, line, column| {
                    let offset = balaur::rune::offset_of(source, line, column);
                    let name = word_at(source, offset);
                    if name.is_empty() {
                        return Ok(Vec::new());
                    }
                    Ok(host
                        .references(key, source, &name)?
                        .iter()
                        .map(|one| {
                            json!({
                                "uri": self.uri_of(&one.file),
                                "range": span(one.line, one.column),
                            })
                        })
                        .collect())
                });
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": items }),
                )?;
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

    /// Run `f` for the file and position a request names, answering `null`
    /// when the file is not one we have. LSP counts from zero and the host
    /// counts from one.
    fn at<T>(
        &self,
        params: &Json,
        f: impl FnOnce(&balaur::rune::RuneHost, &str, &str, usize, usize) -> Result<Vec<T>>,
    ) -> Json
    where
        T: Into<Json>,
    {
        let Some((rel, source, line, column)) = self.locate(params) else {
            return Json::Null;
        };
        match f(&self.host, &rel, &source, line, column) {
            Ok(found) => Json::Array(found.into_iter().map(Into::into).collect()),
            Err(err) => {
                tracing::error!("{rel}: {err:#}");
                Json::Null
            }
        }
    }

    /// `at` for a request answering one value rather than a list.
    fn one(
        &self,
        params: &Json,
        f: impl FnOnce(&balaur::rune::RuneHost, &str, &str, usize, usize) -> Result<Option<Json>>,
    ) -> Json {
        let Some((rel, source, line, column)) = self.locate(params) else {
            return Json::Null;
        };
        match f(&self.host, &rel, &source, line, column) {
            Ok(Some(found)) => found,
            Ok(None) => Json::Null,
            Err(err) => {
                tracing::error!("{rel}: {err:#}");
                Json::Null
            }
        }
    }

    /// `at` for a request about a whole file rather than a position.
    fn whole<T>(
        &self,
        params: &Json,
        f: impl FnOnce(&balaur::rune::RuneHost, &str, &str) -> Result<Vec<T>>,
    ) -> Json
    where
        T: Into<Json>,
    {
        let Some(uri) = params["textDocument"]["uri"].as_str() else {
            return Json::Null;
        };
        let Some(rel) = self.rel_of(uri) else {
            return Json::Null;
        };
        let Some(source) = self.source_of(&rel) else {
            return Json::Null;
        };
        match f(&self.host, &rel, &source) {
            Ok(found) => Json::Array(found.into_iter().map(Into::into).collect()),
            Err(err) => {
                tracing::error!("{rel}: {err:#}");
                Json::Null
            }
        }
    }

    /// The whole file formatted, as the one edit LSP wants: a range covering
    /// everything, replaced. `null` when the source will not parse, which is
    /// what a client should see rather than a mangled buffer.
    fn formatting(&self, params: &Json) -> Json {
        let Some(uri) = params["textDocument"]["uri"].as_str() else {
            return Json::Null;
        };
        let Some(rel) = self.rel_of(uri) else {
            return Json::Null;
        };
        let Some(source) = self.source_of(&rel) else {
            return Json::Null;
        };
        let Ok(formatted) = self.host.format(&rel, &source) else {
            return Json::Null;
        };
        if formatted == source {
            return json!([]);
        }
        // The end is past any real position, which is how LSP says "to the
        // end of the document" without counting its lines.
        json!([{
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": u32::MAX, "character": 0 },
            },
            "newText": formatted,
        }])
    }

    /// The file, its text, and the 1-based position a request names.
    fn locate(&self, params: &Json) -> Option<(String, String, usize, usize)> {
        let uri = params["textDocument"]["uri"].as_str()?;
        let line = params["position"]["line"].as_u64().unwrap_or(0) as usize + 1;
        let column = params["position"]["character"].as_u64().unwrap_or(0) as usize + 1;
        let rel = self.rel_of(uri)?;
        let source = self.source_of(&rel)?;
        Some((rel, source, line, column))
    }

    /// The project-relative path a `file://` URI names, when it is under the
    /// project at all.
    fn rel_of(&self, uri: &str) -> Option<String> {
        let path = Path::new(uri.strip_prefix("file://")?);
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        Some(rel.to_string_lossy().replace('\\', "/"))
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

/// A [`Completion`](balaur::rune::Completion) as an LSP completion item. The
/// doc line is the reference's, so a popup says what the manual says.
fn completion(one: &balaur::rune::Completion) -> Json {
    json!({
        "label": one.label,
        "kind": one.kind.lsp(),
        "detail": one.detail,
        "documentation": one.doc,
        "insertText": one.insert,
    })
}

/// A hover as the markdown a client renders: the name in code, then the
/// signature, then the reference's own doc line.
fn markdown(one: &balaur::rune::Hover) -> String {
    let mut out = format!("```rune\n{}{}\n```", one.title, one.detail);
    if !one.doc.is_empty() {
        out.push_str("\n\n");
        out.push_str(&one.doc);
    }
    out
}

/// A one-character range at a 1-based line and column, which is what a
/// definition and a symbol both want: the point, not the extent.
fn span(line: usize, column: usize) -> Json {
    let start = position(line, column);
    json!({ "start": start, "end": start })
}

/// The whole identifier a byte offset touches, for a request that names a
/// position and means the word there.
fn word_at(source: &str, offset: usize) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let end = source[offset.min(source.len())..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(source.len(), |(i, _)| offset + i);
    let head = &source[..end];
    let start = head
        .char_indices()
        .rev()
        .find(|(_, c)| !is_word(*c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    head[start..].to_string()
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
