//! The URL schemes that open the game (`[android] schemes`): an intent filter
//! each on the template's `org.balaur.platform.Redirect`, which is where a
//! `platform.web_sign_in` page that ends on one lands.

use anyhow::{Context, Result, bail};

/// The activity a scheme's intent filter goes on.
const REDIRECT: &str = "android:name=\"org.balaur.platform.Redirect\"";
const ACTIVITY_END: &str = "</activity>";
const SCHEME: &str = "{scheme}";
/// One scheme's filter, as a browser's link to it is matched.
const FILTER: &str = r#"      <intent-filter>
        <action android:name="android.intent.action.VIEW" />
        <category android:name="android.intent.category.DEFAULT" />
        <category android:name="android.intent.category.BROWSABLE" />
        <data android:scheme="{scheme}" />
      </intent-filter>
"#;

/// `xml` with an intent filter for each of `schemes` on the redirect activity;
/// an error for a scheme no filter matches, or a template with no such activity.
pub(super) fn declare(xml: &str, schemes: &[String]) -> Result<String> {
    if schemes.is_empty() {
        return Ok(xml.to_string());
    }
    let mut filters = String::new();
    for scheme in schemes {
        check(scheme)?;
        filters.push_str(&FILTER.replace(SCHEME, scheme));
    }
    let at = xml.find(REDIRECT).context(
        "[android] schemes needs the template's org.balaur.platform.Redirect activity, \
         which this template predates; scripts/package_runtime.sh writes it",
    )?;
    let end = at
        + xml[at..]
            .find(ACTIVITY_END)
            .context("the template's Redirect activity is unterminated")?;
    let line = xml[..end].rfind('\n').map_or(end, |newline| newline + 1);
    Ok(format!("{}{filters}{}", &xml[..line], &xml[line..]))
}

/// A scheme an intent filter matches: Android compares them case-sensitively,
/// so lowercase, a letter first, then letters, digits, `+`, `-` or `.`.
fn check(scheme: &str) -> Result<()> {
    let shaped = scheme.starts_with(|c: char| c.is_ascii_lowercase())
        && scheme
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c));
    if !shaped {
        bail!(
            "[android] schemes names {scheme:?}, which no intent filter matches: lowercase, \
             a letter first, then letters, digits, +, - or ., e.g. \"com.studio.game\""
        );
    }
    if scheme == "http" || scheme == "https" {
        bail!(
            "[android] schemes names {scheme:?}, the web's own: a page ends on a scheme of \
             the game's, e.g. \"com.studio.game\""
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::AndroidConfig;
    use super::super::tests::{TEMPLATE, named};

    /// The manifest scripts/package_runtime.sh stages, its redirect activity
    /// with no filter of its own.
    const TEMPLATE_WITH_REDIRECT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.balaur.template"
    android:versionCode="1"
    android:versionName="1.0">
  <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="36" />
  <application android:label="Balaur" android:hasCode="true">
    <activity
        android:name="org.balaur.platform.Redirect"
        android:exported="true"
        android:theme="@android:style/Theme.NoDisplay">
    </activity>
  </application>
</manifest>
"#;

    fn with(schemes: &[&str]) -> AndroidConfig {
        AndroidConfig {
            schemes: schemes.iter().map(|&s| s.to_string()).collect(),
            ..named()
        }
    }

    #[test]
    fn a_scheme_is_an_intent_filter_on_the_redirect_activity() {
        let xml = with(&["com.studio.tide", "tide"])
            .manifest(TEMPLATE_WITH_REDIRECT)
            .unwrap();
        for scheme in ["com.studio.tide", "tide"] {
            assert!(
                xml.contains(&format!(r#"<data android:scheme="{scheme}" />"#)),
                "{xml}"
            );
        }
        let redirect = xml.find("org.balaur.platform.Redirect").unwrap();
        let filter = xml.find("<intent-filter>").unwrap();
        assert!(redirect < filter && filter < xml.find("</activity>").unwrap());
        assert!(xml.contains("android.intent.category.BROWSABLE"), "{xml}");
    }

    // A game that names none writes the manifest it always did.
    #[test]
    fn no_scheme_leaves_the_redirect_activity_bare() {
        let xml = named().manifest(TEMPLATE_WITH_REDIRECT).unwrap();
        assert!(!xml.contains("<intent-filter>"), "{xml}");
    }

    #[test]
    fn a_scheme_no_filter_matches_is_refused() {
        for bad in ["Tide", "1tide", "ti de", "https", ""] {
            let err = with(&[bad])
                .manifest(TEMPLATE_WITH_REDIRECT)
                .unwrap_err()
                .to_string();
            assert!(err.contains("[android] schemes"), "{bad}: {err}");
        }
    }

    // An older template has nowhere for the filter to go: a page would end
    // on the scheme and land nowhere.
    #[test]
    fn a_scheme_needs_the_redirect_activity() {
        let err = with(&["tide"]).manifest(TEMPLATE).unwrap_err().to_string();
        assert!(err.contains("Redirect"), "{err}");
    }
}
