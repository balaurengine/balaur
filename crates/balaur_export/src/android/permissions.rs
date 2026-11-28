//! The permissions an Android manifest declares (`[android] permissions`).

use anyhow::{Result, bail};

/// What a game that names none declares: without it no socket opens.
pub(super) const INTERNET: &str = "INTERNET";
/// What a bare permission name is short for.
const PERMISSION_PREFIX: &str = "android.permission.";

/// `xml` with a `uses-permission` for each of `names`, before its
/// `<application>`. One the template already declares for its own shims
/// (VIBRATE, POST_NOTIFICATIONS) is declared once.
///
/// # Errors
/// When a name is not one Android could declare, or the template has no
/// `<application>`.
pub(super) fn declare(xml: &str, names: &[String]) -> Result<String> {
    let mut lines = String::new();
    for name in names {
        let full = permission_name(name)?;
        let declared = format!("<uses-permission android:name=\"{full}\"");
        if xml.contains(&declared) || lines.contains(&declared) {
            continue;
        }
        lines.push_str("  <uses-permission android:name=\"");
        lines.push_str(&full);
        lines.push_str("\" />\n");
    }
    super::before_application(xml, &lines)
}

/// A permission as the manifest names it: `INTERNET` is
/// `android.permission.INTERNET`; a dotted name is taken whole.
///
/// # Errors
/// When the name is not one Android could declare, which a store would
/// refuse and a device ignore.
fn permission_name(name: &str) -> Result<String> {
    let constant = |part: &str| {
        !part.is_empty()
            && part.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && part
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    };
    if constant(name) {
        return Ok(format!("{PERMISSION_PREFIX}{name}"));
    }
    let parts: Vec<&str> = name.split('.').collect();
    let package_part = |part: &&str| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if parts.len() > 1
        && parts[..parts.len() - 1].iter().all(package_part)
        && parts.last().is_some_and(|last| constant(last))
    {
        return Ok(name.to_string());
    }
    bail!(
        "[android] permissions names {name:?}, which is not an Android permission: \
         INTERNET, or a whole name such as android.permission.POST_NOTIFICATIONS"
    )
}

#[cfg(test)]
mod tests {
    use super::super::AndroidConfig;
    use super::super::test_config::{TEMPLATE, named};

    #[test]
    fn a_permission_is_a_bare_constant_or_a_whole_name() {
        let config = AndroidConfig {
            permissions: vec![
                "POST_NOTIFICATIONS".into(),
                "com.android.vending.BILLING".into(),
            ],
            ..named()
        };
        let xml = config.manifest(TEMPLATE).unwrap();
        assert!(
            xml.contains(r#""android.permission.POST_NOTIFICATIONS""#),
            "{xml}"
        );
        assert!(xml.contains(r#""com.android.vending.BILLING""#), "{xml}");
        for wrong in ["internet", "android.permission.", "a b", "\"/><x"] {
            let config = AndroidConfig {
                permissions: vec![wrong.into()],
                ..named()
            };
            let err = config.manifest(TEMPLATE).unwrap_err().to_string();
            assert!(err.contains("not an Android permission"), "{wrong}: {err}");
        }
    }
}
