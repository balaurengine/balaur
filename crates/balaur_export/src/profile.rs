//! A provisioning profile, read for what it grants: the entitlements a bundle
//! signed against it may carry.
//!
//! A profile is a CMS envelope around an XML plist, signed by Apple. The
//! export reads the plist and never checks the signature: the device does
//! that at install, and by then the profile is the one this export embedded.
//! What the export does check is that every entitlement the game's
//! capabilities write is one the profile grants, so a profile made before the
//! App ID had Push fails here, naming the capability, rather than at install.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

/// Where the plist starts inside the envelope, and where it ends.
const PLIST_START: &[u8] = b"<?xml";
const PLIST_END: &[u8] = b"</plist>";
/// The profile's key holding what it grants.
const ENTITLEMENTS: &str = "Entitlements";
const DICT: &str = "dict";
const KEY: &str = "key";
const STRING: &str = "string";

/// What a provisioning profile grants.
#[derive(Debug)]
pub(crate) struct Profile {
    /// Each entitlement the profile grants, with its value where that value
    /// is one string (`aps-environment`'s `development` or `production`).
    granted: BTreeMap<String, Option<String>>,
}

impl Profile {
    /// The profile at `path`: a `.mobileprovision` or a `.provisionprofile`.
    pub(crate) fn read(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading the provisioning profile {}", path.display()))?;
        Self::parse(&bytes)
            .with_context(|| format!("reading the provisioning profile {}", path.display()))
    }

    /// A profile's bytes, the envelope and all.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let start =
            find(bytes, PLIST_START).context("it holds no plist: is it a provisioning profile?")?;
        let end = find(&bytes[start..], PLIST_END)
            .map(|at| start + at + PLIST_END.len())
            .context("its plist is cut short")?;
        let text = std::str::from_utf8(&bytes[start..end]).context("its plist is not UTF-8")?;
        let document = roxmltree::Document::parse_with_options(
            text,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..roxmltree::ParsingOptions::default()
            },
        )
        .context("its plist does not parse")?;
        let top = document
            .root_element()
            .children()
            .find(|node| node.has_tag_name(DICT))
            .context("its plist holds no dictionary")?;
        let entitlements = entry(top, ENTITLEMENTS)
            .filter(|node| node.has_tag_name(DICT))
            .context("it grants no entitlements")?;
        let granted = pairs(entitlements)
            .map(|(key, value)| {
                let text = value
                    .has_tag_name(STRING)
                    .then(|| value.text().unwrap_or_default().to_string());
                (key.to_string(), text)
            })
            .collect();
        Ok(Self { granted })
    }

    /// Whether the profile grants `entitlement`, whatever its value.
    pub(crate) fn grants(&self, entitlement: &str) -> bool {
        self.granted.contains_key(entitlement)
    }

    /// The one string the profile grants `entitlement` with, if it is one.
    pub(crate) fn value(&self, entitlement: &str) -> Option<&str> {
        self.granted.get(entitlement)?.as_deref()
    }
}

/// A plist dictionary's keys, each with the element after it.
fn pairs<'a, 'input>(
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
fn entry<'a, 'input>(
    dict: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    pairs(dict)
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::Profile;

    /// A profile's plist inside bytes that stand for its CMS envelope, as
    /// Apple's portal hands one out: what the export reads is the plist.
    pub(crate) fn profile_granting(entitlements: &str) -> Vec<u8> {
        let mut bytes = vec![0x30, 0x80, 0x06, 0x09, 0x2a, 0x86, 0x48];
        bytes.extend_from_slice(
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>AppIDName</key><string>Game</string>
  <key>DeveloperCertificates</key><array><data>AAAA</data></array>
  <key>Entitlements</key>
  <dict>
    <key>com.apple.application-identifier</key><string>AB12CD34EF.com.studio.game</string>
    <key>keychain-access-groups</key><array><string>AB12CD34EF.*</string></array>
{entitlements}  </dict>
  <key>Name</key><string>Game Developer ID</string>
</dict>
</plist>"#
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(&[0xa0, 0x82, 0x0d, 0x00]);
        bytes
    }

    #[test]
    fn a_profile_grants_what_its_entitlements_name() {
        let profile = Profile::parse(&profile_granting(
            "    <key>com.apple.developer.applesignin</key><array><string>Default</string></array>\n\
             \x20   <key>com.apple.developer.aps-environment</key><string>production</string>\n",
        ))
        .unwrap();
        assert!(profile.grants("com.apple.developer.applesignin"));
        assert!(profile.grants("keychain-access-groups"));
        assert!(!profile.grants("com.apple.developer.game-center"));
        assert!(!profile.grants("Name"), "a key outside Entitlements");
        assert_eq!(
            profile.value("com.apple.developer.aps-environment"),
            Some("production")
        );
        assert_eq!(profile.value("com.apple.developer.applesignin"), None);
    }

    // A profile Apple signed, where one is named: the CMS envelope around the
    // plist is real, which the stand-in above only stands for.
    #[test]
    fn a_real_profile_parses_where_one_is_named() {
        let Some(path) = std::env::var_os("BALAUR_TEST_PROFILE") else {
            return;
        };
        let profile = Profile::read(std::path::Path::new(&path)).unwrap();
        assert!(
            profile.grants("com.apple.application-identifier"),
            "{profile:?}"
        );
    }

    #[test]
    fn a_file_that_is_no_profile_is_refused() {
        let err = Profile::parse(b"\x00\x01 not a profile").unwrap_err();
        assert!(err.to_string().contains("no plist"), "{err}");
        let none =
            b"<?xml version=\"1.0\"?><plist><dict><key>Name</key><string>x</string></dict></plist>";
        let err = Profile::parse(none).unwrap_err();
        assert!(err.to_string().contains("no entitlements"), "{err}");
    }
}
