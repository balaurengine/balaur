//! The `[export]`, `[android]` and `[apple]` tables, declared the way every
//! other table in `project.toml` is.
//!
//! What this buys: the export sheet stops being a screen with its own
//! configuration and becomes a button, since the settings screen already
//! groups, searches and explains anything declared here. A key gets an
//! `[override.<tag>]` for free, which is what lets one project sign a
//! download one way and a store build another.
//!
//! The schema and the structs it describes have to agree. `deny_unknown_fields`
//! catches a key here that the struct does not carry; `settings::unknown`
//! catches a key the struct carries and this does not.

use balaur::ComponentDef;
use balaur::Engine;
use balaur::settings::{Scope, define_group};

/// Declare all three. Called by whoever installs the exporter, so a game that
/// cannot export does not carry export settings it can do nothing with.
pub fn declare(eng: &Engine) {
    let parse = |name: &str, text: &str| ComponentDef::parse_schema(name, text);
    define_group(
        eng,
        "export",
        Scope::Project,
        &parse(
            "settings.export",
            r#"
output = { type = "string", default = "", order = 1, help = "A project-relative directory; each target gets a subdirectory of it. Empty exports where the command stands." }
strip = { type = "bool", default = false, order = 2, help = "Drop an asset no scene, script or keep-glob names. Off by default: a script may compute a path this cannot see, and losing an asset is worse than shipping one." }
keep = { type = "strings", default = [], order = 3, help = "Globs an export keeps whatever else it decides, for the paths a script builds at run time." }
images = { type = "enum", default = "keep", options = ["keep", "png", "webp", "smallest", "quantised"], order = 4, help = "How an image is re-encoded on the way into the pack. Every mode keeps the size; quantised is the one that does not keep the pixels." }
images_quality = { type = "int", default = 70, min = 0, max = 100, order = 5, help = "imagequant's quality target, which images = \"quantised\" reads and every other mode ignores." }
fonts = { type = "enum", default = "keep", options = ["keep", "subset"], order = 6, help = "Whether a font is cut down to the characters the project's scenes and scripts name." }
font_ranges = { type = "strings", default = [], order = 7, help = "Code points a subset font keeps beyond the ones found in the project, as first-last hex ranges (\"0020-00FF\"), for text from a server or typed by a player." }
font_keep = { type = "strings", default = [], order = 8, help = "Faces that ship whole however fonts is set, as globs: the one a text field draws with cannot be subset to the characters this project happens to contain." }
audio = { type = "enum", default = "keep", options = ["keep", "flac", "vorbis"], order = 9, help = "How uncompressed audio is re-encoded. flac keeps every sample; vorbis does not." }
audio_quality = { type = "float", default = 0.5, min = -0.1, max = 1.0, order = 10, help = "libvorbis's quality, which audio = \"vorbis\" reads and every other mode ignores." }
macos_identity = { type = "string", default = "", order = 20, help = "Developer ID Application: … for a download, Apple Distribution: … for the Mac App Store. The password behind it is read from the environment, never from here." }
notarize = { type = "bool", default = false, order = 21, help = "Submit to Apple's notary service after signing, and staple the ticket." }
ios_identity = { type = "string", default = "", order = 22, help = "The identity an iOS build is signed with." }
ios_profile = { type = "string", default = "", order = 23, help = "A project-relative .mobileprovision, copied into the bundle." }
android_keystore = { type = "string", default = "", order = 24, help = "A project-relative keystore, or empty for Android's debug identity." }
android_key = { type = "string", default = "", order = 25, help = "Which key in that keystore signs." }
bundletool = { type = "string", default = "", order = 26, help = "Where bundletool.jar is. Empty looks at BALAUR_BUNDLETOOL and then beside the SDK; Google ships it on its own, not in the SDK." }
windows_certificate = { type = "string", default = "", order = 27, help = "A project-relative .pfx, or an Azure Trusted Signing metadata file when the key lives in a cloud HSM." }
windows_timestamp_url = { type = "string", default = "http://timestamp.digicert.com", order = 28, help = "The timestamp authority a Windows signature is countersigned by." }
"#,
        ),
    );
    define_group(
        eng,
        "android",
        Scope::Project,
        &parse(
            "settings.android",
            r#"
application_id = { type = "string", default = "", order = 1, help = "The identifier Play resolves an OAuth client, a licence and an update against. Empty keeps the invented org.balaur.<name>." }
label = { type = "string", default = "", order = 2, help = "The name under the icon. Empty means the project's own." }
version = { type = "string", default = "1.0", order = 3, help = "versionName: what a player is shown." }
version_code = { type = "int", default = 1, min = 1, max = 2100000000, order = 4, help = "versionCode: what Play orders updates by, and the only one it reads." }
min_sdk = { type = "int", default = 0, min = 0, max = 40, order = 5, help = "The API floor. 0 defers to the template's own, which is what its libraries were built against." }
target_sdk = { type = "int", default = 35, min = 21, max = 40, order = 6, help = "The API this game says it was written for." }
abis = { type = "flags", default = [], options = ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"], order = 7, help = "Which of the template's ABIs the export keeps. None named means every one it carries, so a game that says nothing ships everywhere." }
"#,
        ),
    );
    define_group(
        eng,
        "apple",
        Scope::Project,
        &parse(
            "settings.apple",
            r#"
bundle_id = { type = "string", default = "", order = 1, help = "The identifier registered in App Store Connect. Empty means the exporter keeps its invented org.balaur.<name>." }
team = { type = "string", default = "", order = 2, help = "The ten-character team identifier. Nothing expands it, so an entitlement carrying the prefix needs the real value." }
display_name = { type = "string", default = "", order = 3, help = "The name under the icon. Empty means the project's own." }
version = { type = "string", default = "1.0", order = 4, help = "CFBundleShortVersionString: what a player is shown." }
build = { type = "string", default = "1", order = 5, help = "CFBundleVersion: what the store orders uploads by." }
min_os = { type = "string", default = "15.0", order = 6, help = "MinimumOSVersion on iOS. A plist may not claim less than the binary was built for." }
min_macos = { type = "string", default = "12.0", order = 7, help = "LSMinimumSystemVersion on macOS." }
category = { type = "string", default = "", order = 8, help = "LSApplicationCategoryType, macOS only." }
capabilities = { type = "flags", default = [], options = ["applesignin", "game-center", "icloud-kv", "in-app-purchase"], order = 9, help = "What the bundle declares it uses. Each writes its own entitlement and its own minimum OS." }
"#,
        ),
    );
}
