//! Reading an XML plist: a provisioning profile's, a template's `Info.plist`
//! for the keys an export keeps, and the bundle's own for the keys App Store
//! Connect requires.

use anyhow::{Context, Result};

pub(crate) const DICT: &str = "dict";
const KEY: &str = "key";
pub(crate) const STRING: &str = "string";
const ARRAY: &str = "array";
const INTEGER: &str = "integer";
const REAL: &str = "real";
const TRUE: &str = "true";

/// A plist document, its DTD line allowed.
pub(crate) fn parse(text: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .context("its plist does not parse")
}

/// The dictionary a document holds at its top.
pub(crate) fn top<'a, 'input>(
    document: &'a roxmltree::Document<'input>,
) -> Result<roxmltree::Node<'a, 'input>> {
    document
        .root_element()
        .children()
        .find(|node| node.has_tag_name(DICT))
        .context("its plist holds no dictionary")
}

/// A plist dictionary's keys, each with the element after it.
pub(crate) fn pairs<'a, 'input>(
    dict: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = (&'a str, roxmltree::Node<'a, 'input>)> {
    let mut elements = dict.children().filter(roxmltree::Node::is_element);
    std::iter::from_fn(move || {
        loop {
            let key = elements.next()?;
            if !key.has_tag_name(KEY) {
                continue;
            }
            let value = elements.next()?;
            return Some((key.text().unwrap_or_default(), value));
        }
    })
}

/// The value under `name` in a plist dictionary.
pub(crate) fn entry<'a, 'input>(
    dict: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    pairs(dict)
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// A top-level value, as far as a check of it reads.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// A string or a number, as written.
    Text(String),
    /// An array's items as written, or a dictionary's keys set to `true`:
    /// `UIRequiredDeviceCapabilities` may be either.
    Items(Vec<String>),
    True,
    Other,
}

impl Value {
    fn of(node: roxmltree::Node<'_, '_>) -> Self {
        let text = |node: roxmltree::Node<'_, '_>| node.text().unwrap_or_default().to_string();
        match node.tag_name().name() {
            STRING | INTEGER | REAL => Self::Text(text(node)),
            ARRAY => Self::Items(
                node.children()
                    .filter(roxmltree::Node::is_element)
                    .map(text)
                    .collect(),
            ),
            DICT => Self::Items(
                pairs(node)
                    .filter(|(_, value)| value.has_tag_name(TRUE))
                    .map(|(key, _)| key.to_string())
                    .collect(),
            ),
            TRUE => Self::True,
            _ => Self::Other,
        }
    }

    /// The string, where this is one that is not empty.
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) if !text.is_empty() => Some(text),
            _ => None,
        }
    }

    /// Whether this is a list naming `item`.
    pub(crate) fn lists(&self, item: &str) -> bool {
        matches!(self, Self::Items(items) if items.iter().any(|one| one == item))
    }
}

/// Every top-level key of the plist `text`, in order, with its value.
pub(crate) fn top_level(text: &str) -> Result<Vec<(String, Value)>> {
    let document = parse(text)?;
    Ok(pairs(top(&document)?)
        .map(|(key, value)| (key.to_string(), Value::of(value)))
        .collect())
}

/// The string under `key` at the top of the plist at `path`.
pub(crate) fn string_at(path: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    top_level(&text)
        .ok()?
        .into_iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| value.text().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::{Value, top_level};

    #[test]
    fn a_plist_reads_as_its_top_level_keys_and_what_each_holds() {
        let keys = top_level(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\
             <key>DTSDKName</key><string>iphoneos27.1</string>\
             <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>\
             <key>UIRequiredDeviceCapabilities</key><dict><key>arm64</key><true/>\
             <key>metal</key><false/></dict>\
             <key>UIRequiresFullScreen</key><true/>\
             <key>UILaunchScreen</key><dict/></dict></plist>",
        )
        .unwrap();
        assert_eq!(
            keys,
            [
                ("DTSDKName".into(), Value::Text("iphoneos27.1".into())),
                (
                    "UIDeviceFamily".into(),
                    Value::Items(vec!["1".into(), "2".into()])
                ),
                (
                    "UIRequiredDeviceCapabilities".into(),
                    Value::Items(vec!["arm64".into()])
                ),
                ("UIRequiresFullScreen".into(), Value::True),
                ("UILaunchScreen".into(), Value::Items(Vec::new())),
            ]
        );
        assert!(keys[2].1.lists("arm64") && !keys[2].1.lists("metal"));
    }
}
