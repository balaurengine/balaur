//! Shared setup for the benchmarks: build a project on disk, boot an app on a
//! chosen backend, attach scripts to N nodes.
//!
//! Everything here runs headless. There is no window and no render backend, so
//! a measurement is the engine, not the GPU driver.

use std::path::Path;

use anyhow::Result;
use balaur_core::{App, AppConfig, ScriptHostFactory};

/// Which backend a benchmark runs on. The same scenario is measured on both,
/// which is the only way to say what a language actually costs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Luau,
    Rune,
}

impl Backend {
    pub const ALL: [Self; 2] = [Self::Luau, Self::Rune];

    pub fn name(self) -> &'static str {
        match self {
            Self::Luau => "luau",
            Self::Rune => "rune",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Luau => "luau",
            Self::Rune => "rn",
        }
    }

    fn factory(self) -> ScriptHostFactory {
        match self {
            Self::Luau => balaur_script_luau::factory(),
            Self::Rune => balaur_script_rune::factory(),
        }
    }
}

/// A project directory that lives as long as the benchmark holds it.
pub struct Project {
    pub dir: tempfile::TempDir,
}

impl Project {
    /// Write a project whose single script is `source`, in `backend`'s syntax.
    ///
    /// # Errors
    /// If the directory cannot be written.
    pub fn new(backend: Backend, source: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("project.toml"),
            format!(
                "name = \"bench\"\nmain_scene = \"main.toml\"\nlanguage = \"{}\"\n",
                backend.name()
            ),
        )?;
        std::fs::write(
            dir.path().join("main.toml"),
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
        )?;
        std::fs::write(
            dir.path().join(format!("s.{}", backend.extension())),
            source,
        )?;
        Ok(Self { dir })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// Boot an app with the standard plugins on `backend`.
///
/// # Errors
/// If the app or its plugins fail to build.
pub fn app(backend: Backend, project: &Project) -> Result<App> {
    let mut config = AppConfig::dev(project.path().to_string_lossy().as_ref());
    config.watch = false;
    config.scripts = Some(backend.factory());
    let mut app = balaur::standard_app(config)?;
    app.load_project()?;
    Ok(app)
}

/// Attach the project's script to `count` fresh nodes.
///
/// # Errors
/// If a node cannot be created or the script fails to attach.
pub fn attach_many(app: &App, backend: Backend, count: usize) -> Result<Vec<hecs::Entity>> {
    let host = app
        .engine
        .scripts()
        .ok_or_else(|| anyhow::anyhow!("no script backend"))?;
    let root = app.engine.root();
    let path = format!("s.{}", backend.extension());
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let entity =
            balaur_core::scene::spawn_node(&mut app.engine.world_mut(), &format!("n{i}"), root);
        host.attach(balaur_core::node_id_of(entity), &path)?;
        out.push(entity);
    }
    Ok(out)
}
