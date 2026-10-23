//! A Language Server, so an editor outside Balaur gets the diagnostics the
//! Balaur editor's Problems dock shows.
//!
//! It speaks LSP on stdin and stdout and asks the same `LanguageTools` the
//! editor asks, so there is one definition of what is wrong with a script.
//!
//! # Not a system in the engine
//!
//! The DAP server ([`balaur_core::dap`]) lives inside a running game because
//! debugging is a conversation with one: a breakpoint has to land at a point
//! in the frame. Checking has no such tie — it needs the engine's script API
//! and nothing else — so this is a process an editor spawns, boots the project
//! once, and then blocks on stdin. No threads, no frame loop, no game.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use balaur_core::Engine;
use balaur_script::{LanguageTools, ScriptHost};
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
    let mut server = Server {
        root,
        scripts: app
            .engine
            .script_host()
            .context("this build runs no scripts")?,
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
    scripts: Rc<dyn ScriptHost<Engine>>,
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
                            "workspaceSymbolProvider": true,
                            "referencesProvider": true,
                            "renameProvider": true,
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
            // Every other request answers with one value, or `null`: a
            // client that waits for an answer hangs without one.
            _ if id.is_some() => {
                let result = self.answer(method, &message["params"]);
                write_message(
                    writer,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                )?;
            }
            _ => {}
        }
        Ok(false)
    }

    /// The answer to one request, or `Json::Null` for a method we do not
    /// serve or a file no language answers for.
    fn answer(&self, method: &str, params: &Json) -> Json {
        if method == "workspace/symbol" {
            return self.workspace_symbols(params);
        }
        let Some((rel, source, line, column)) = self.locate(params) else {
            return Json::Null;
        };
        let Some(tools) = self.scripts.tools_for(&rel) else {
            return Json::Null;
        };
        match method {
            "textDocument/completion" => tools
                .complete(&rel, &source, line, column)
                .iter()
                .map(|c| {
                    json!({
                        "label": c.label,
                        "kind": item_kind(c.kind),
                        "detail": c.detail,
                        "insertText": c.label,
                    })
                })
                .collect(),
            "textDocument/hover" => tools.hover(&rel, &source, line, column).map_or(Json::Null, |h| {
                json!({ "contents": { "kind": "markdown", "value": format!("```bs\n{h}\n```") } })
            }),
            "textDocument/definition" => tools
                .definition(&rel, &source, line, column)
                .map_or(Json::Null, |d| json!({ "uri": self.uri_of(&d.file), "range": span(d.line, d.column) })),
            _ => self.answer_more(method, params, tools, &rel, &source, (line, column)),
        }
    }

    /// The rest of [`Server::answer`]: the outline, the call, and the edits.
    fn answer_more(
        &self,
        method: &str,
        params: &Json,
        tools: &dyn LanguageTools,
        rel: &str,
        source: &str,
        (line, column): (usize, usize),
    ) -> Json {
        let word = || word_at(source, offset_of(source, line, column));
        match method {
            "textDocument/signatureHelp" => tools.signature(rel, source, line, column).map_or(Json::Null, |(sig, active)| {
                json!({ "signatures": [{ "label": sig }], "activeSignature": 0, "activeParameter": active })
            }),
            "textDocument/documentSymbol" => tools
                .symbols(rel, source)
                .iter()
                .map(|one| {
                    json!({
                        "name": one.name,
                        "kind": symbol_kind(one.kind),
                        "detail": one.detail,
                        "range": span(one.line.max(1), one.column),
                        "selectionRange": span(one.line.max(1), one.column),
                    })
                })
                .collect(),
            "textDocument/references" => tools
                .references(rel, source, &word())
                .iter()
                .map(|one| json!({ "uri": self.uri_of(&one.file), "range": span(one.line, one.column) }))
                .collect(),
            "textDocument/rename" => params["newName"]
                .as_str()
                .and_then(|to| tools.rename(rel, source, &word(), to))
                .map_or(Json::Null, |text| {
                    let mut changes = serde_json::Map::new();
                    changes.insert(self.uri_of(rel), json!([whole_file(&text)]));
                    json!({ "changes": changes })
                }),
            "textDocument/formatting" => tools.format(rel, source).map_or(Json::Null, |text| {
                if text == source { json!([]) } else { json!([whole_file(&text)]) }
            }),
            _ => Json::Null,
        }
    }

    /// Every symbol in the project whose name contains the query, as the flat
    /// `SymbolInformation` list `workspace/symbol` wants. The per-file answer
    /// over every script a scene attaches: there is no project-wide index, and
    /// a project is a few dozen files.
    fn workspace_symbols(&self, params: &Json) -> Json {
        let query = params["query"].as_str().unwrap_or("").to_lowercase();
        let mut out = Vec::new();
        for rel in balaur::scene_scripts(&self.root) {
            let (Some(source), Some(tools)) = (self.source_of(&rel), self.scripts.tools_for(&rel))
            else {
                continue;
            };
            for one in tools.symbols(&rel, &source) {
                if !query.is_empty() && !one.name.to_lowercase().contains(&query) {
                    continue;
                }
                out.push(json!({
                    "name": one.name,
                    "kind": symbol_kind(one.kind),
                    "containerName": rel,
                    "location": {
                        "uri": self.uri_of(&rel),
                        "range": span(one.line.max(1), one.column),
                    },
                }));
            }
        }
        Json::Array(out)
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
    /// in. A root is a script a scene attaches; a file it `use`s is checked
    /// with it.
    fn publish(&mut self, writer: &mut impl Write) -> Result<()> {
        let mut by_file: std::collections::BTreeMap<String, Vec<Json>> =
            std::collections::BTreeMap::new();
        for rel in balaur::scene_scripts(&self.root) {
            let Some(source) = self.source_of(&rel) else {
                continue;
            };
            let found: Vec<Json> = match self.scripts.tools_for(&rel) {
                Some(tools) => tools.check(&rel, &source).iter().map(diagnostic).collect(),
                None => vec![diagnostic(&balaur_script::Diagnostic {
                    line: 1,
                    column: 1,
                    severity: "error",
                    message: format!("{rel} is not a script this engine runs"),
                })],
            };
            if !found.is_empty() {
                by_file.entry(rel.clone()).or_default().extend(found);
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
        let path = if balaur_core::files::rooted(Path::new(file)) {
            PathBuf::from(file)
        } else {
            self.root.join(file)
        };
        format!("file://{}", path.to_string_lossy())
    }
}

/// The LSP `SymbolKind` for what a script declares.
fn symbol_kind(kind: &str) -> u8 {
    match kind {
        "function" | "method" => 12,
        "struct" => 23,
        _ => 7,
    }
}

/// A one-character range at a 1-based line and column, which is what a
/// definition and a symbol both want: the point, not the extent.
fn span(line: usize, column: usize) -> Json {
    let start = position(line, column);
    json!({ "start": start, "end": start })
}

/// The byte offset of a 1-based line and column, the column in characters.
fn offset_of(source: &str, line: usize, column: usize) -> usize {
    let mut at = 0;
    for (n, text) in source.split('\n').enumerate() {
        if n + 1 == line.max(1) {
            let index = text.char_indices().nth(column.saturating_sub(1));
            return at + index.map_or(text.len(), |(i, _)| i);
        }
        at += text.len() + 1;
    }
    source.len()
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

/// One edit replacing the whole file: the end is past any real position,
/// which is how LSP says "to the end" without counting lines.
fn whole_file(text: &str) -> Json {
    json!({
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": u32::MAX, "character": 0 },
        },
        "newText": text,
    })
}

/// The LSP `CompletionItemKind` for a neutral item's kind.
fn item_kind(kind: &str) -> u8 {
    match kind {
        "module" => 9,
        "function" => 3,
        "method" => 2,
        "field" => 5,
        "property" => 10,
        "variable" => 6,
        "struct" => 22,
        "constant" => 21,
        _ => 14,
    }
}

/// A diagnostic as LSP has it: lines and characters from zero, the host's
/// from one.
fn diagnostic(one: &balaur_script::Diagnostic) -> Json {
    let start = position(one.line, one.column);
    json!({
        "range": { "start": start, "end": start },
        "severity": if one.severity == "error" { 1 } else { 2 },
        "source": "balaur",
        "message": one.message,
    })
}

fn notification(uri: &str, diagnostics: &[Json]) -> Json {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics },
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
