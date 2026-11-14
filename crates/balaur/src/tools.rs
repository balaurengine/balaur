//! The script modules a tool calls and a game does not: `debugger`, and what
//! `script` asks about a file. No runtime registers them; the binary that
//! carries the editor does, beside its own `project` and `export`.

use std::path::PathBuf;

use anyhow::Result;

const DEBUGGER: &str = "debugger";
#[cfg(feature = "bs-backend")]
const SCRIPT: &str = "script";
const TOOLS: &str = "tools";

/// `debugger.*` and `script.*` for scripts, each the editor's, and where the
/// editor project is: its scripts may call them, as may a file under a game's
/// or an addon's `editor/` folder ([`crate::file_api::editors_script`]).
pub struct ToolsPlugin {
    editor: Option<PathBuf>,
}

impl ToolsPlugin {
    /// `editor` is the project the binary runs as the editor; `None` when it
    /// has none, and then only `editor/` folders may call the tools.
    #[must_use]
    pub fn new(editor: Option<PathBuf>) -> Self {
        Self { editor }
    }
}

impl balaur_plugin::Plugin for ToolsPlugin {
    fn name(&self) -> &'static str {
        TOOLS
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        if let Some(editor) = self.editor.take() {
            reg.insert_resource(crate::file_api::EditorRoot(editor));
        }
        let mut debugger = reg.script_module(DEBUGGER)?.editor_only();
        balaur_debug::debugger_api::install_debugger_api(&mut debugger);
        #[cfg(feature = "bs-backend")]
        balaur_script_bs::install_tool_verbs(&mut reg.script_module(SCRIPT)?.editor_only());
        Ok(())
    }
}
