//! The asset types an app is told about.

use super::App;
use anyhow::Result;

impl App {
    /// Register a parser for one asset type (see `balaur_core::assets`).
    ///
    /// The mirror of [`Self::register_component`]: core learns the name and
    /// nothing else, the parser returns an object only its plugin
    /// understands, and `assets::load_typed` hands it back downcast.
    ///
    /// `directory` is the project-relative folder files of this type belong in
    /// (`animations`). A tool promoting an inline definition to a file needs
    /// somewhere to put it and the schema does not carry that; an empty string
    /// means the type has no home and cannot be promoted.
    ///
    /// `doc` describes the definition table for the generated reference:
    /// markdown, a paragraph and a TOML example.
    pub fn register_asset_type(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        parse: impl Fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>> + 'static,
    ) -> &mut Self {
        self.push_asset_type(name, directory, doc, None, None, Box::new(parse))
    }

    /// An asset type with no schema whose parser reads closed keys beside free
    /// names: `check` refuses a key no level takes, where the definition is
    /// named and again before `parse` sees it (`assets::refuse_keys`).
    pub fn register_checked_asset(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        check: crate::assets::AssetCheckFn,
        parse: impl Fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>> + 'static,
    ) -> &mut Self {
        self.push_asset_type(name, directory, doc, None, Some(check), Box::new(parse))
    }

    /// An asset type whose definition is a record of `schema`'s properties:
    /// `parse` is handed the definition with every property at its value or
    /// its default, and an Inspector draws it as it draws a component.
    pub fn register_record_asset(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        schema: std::rc::Rc<crate::components::Schema>,
        parse: impl Fn(&toml::Value) -> Result<std::rc::Rc<dyn std::any::Any>> + 'static,
    ) -> &mut Self {
        self.push_asset_type(name, directory, doc, Some(schema), None, Box::new(parse))
    }

    fn push_asset_type(
        &mut self,
        name: &str,
        directory: &str,
        doc: &'static str,
        schema: Option<std::rc::Rc<crate::components::Schema>>,
        check: Option<crate::assets::AssetCheckFn>,
        parse: crate::assets::AssetParseFn,
    ) -> &mut Self {
        let registry = self.engine.resource::<crate::assets::AssetTypeRegistry>();
        registry.borrow_mut().0.push((
            name.to_string(),
            crate::assets::AssetType {
                parse,
                directory: directory.to_string(),
                doc,
                schema,
                check,
            },
        ));
        self
    }
}
