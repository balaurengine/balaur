//! The `[export]`, `[android]`, `[apple]`, `[windows]` and `[steam]` tables,
//! declared the way every other table in `project.toml` is.
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

use crate::vocabulary::words;
use balaur::ComponentDef;
use balaur::Engine;
use balaur::settings::{Scope, define_group};

/// The `[export]` keys, as the editor's settings panel draws them. One copy,
/// so a mode offered here is one [`crate::config::ExportConfig`] can read.
pub(crate) const EXPORT_SCHEMA: &str = r#"
runtime = { type = "enum", default = "auto", options = ["auto", "full", "2d", "3d", "bare"], order = 0, help = "The runtime a game is put on. 2d and 3d carry one physics world each and bare carries neither, so each weighs less than full, and auto takes the smallest the scenes and scripts fit, or the full one where only that is on hand; name one where a script builds a component's name at run time. A server target keeps its own. Per target through that target's override." }
output = { type = "string", default = "", order = 1, help = "A project-relative directory; each target gets a subdirectory of it. Empty exports where the command stands." }
strip = { type = "bool", default = false, order = 2, help = "Drop an asset no scene, script or include glob names. Off by default: a script may compute a path this cannot see, and losing an asset is worse than shipping one." }
tags = { type = "list", of = { type = "string" }, default = [], order = 11, help = "Names this build answers to besides its platform's, such as demo or store. An override or an asset variant may be written for any of them; per target, set it under that target's override." }
include = { type = "list", of = { type = "string" }, default = [], order = 3, help = "Globs an export includes whatever else it decides, for the paths a script builds at run time." }
image_recode = { type = "enum", default = "webp", options = ["original", "webp", "quantized"], order = 4, help = "How an image is re-encoded on the way into the pack, kept only where it comes out smaller. Every mode keeps the size; quantized is the one that does not keep the pixels." }
image_quality = { type = "int", default = 80, min = 0, max = 100, order = 5, help = "imagequant's quality target, which image_recode = \"quantized\" reads and every other mode ignores." }
max_size = { type = "int", default = 0, min = 0, max = 16384, order = 12, help = "The longest side an image ships at, in pixels; 0 keeps every size. Set it per target to ship a phone a smaller art set. Pixel art and bitmap font pages are never capped, and a sprite keeps the size it was drawn at." }
font_recode = { type = "enum", default = "original", options = ["original", "subset"], order = 6, help = "Whether a font is cut down to the characters the project's scenes and scripts name." }
font_ranges = { type = "list", of = { type = "string" }, default = [], order = 7, help = "Code points a subset font keeps beyond the ones found in the project, as first-last hex ranges (\"0020-00FF\"), for text from a server or typed by a player." }
font_original = { type = "list", of = { type = "string" }, default = [], order = 8, help = "Faces that ship whole however font_recode is set, as globs: the one a text field draws with cannot be subset to the characters this project happens to contain." }
audio_recode = { type = "enum", default = "flac", options = ["original", "flac", "vorbis"], order = 9, help = "How uncompressed audio is re-encoded, kept only where it comes out smaller. flac keeps every sample; vorbis does not." }
audio_quality = { type = "float", default = 0.5, min = -0.1, max = 1.0, order = 10, help = "libvorbis's quality, which audio_recode = \"vorbis\" reads and every other mode ignores." }
later = { type = "list", of = { type = "string" }, default = [], order = 14, help = "Scenes, as globs, whose assets a web export ships in a second file the page fetches after the game starts. A switch to one waits until that file has arrived; no other scene may name what only these do." }
native_scripts = { type = "enum", default = "auto", options = ["auto", "on", "off"], order = 13, help = "Ship the scripts as native code, built from their C with Xcode for an Apple target or clang and lld for any. auto falls back to the target's engine when no compiler is found, on fails the export instead, off never builds it. Per target through that target's override." }
"#;

pub fn declare(eng: &Engine) {
    let parse = |name: &str, text: &str| ComponentDef::parse_schema(name, text);
    define_group(
        eng,
        words::config::EXPORT,
        Scope::Project,
        &parse("settings.export", EXPORT_SCHEMA),
    );
    define_group(
        eng,
        words::targets::ANDROID,
        Scope::Project,
        &parse(
            "settings.android",
            r#"
application_id = { type = "string", default = "", order = 1, help = "The identifier Play resolves an OAuth client, a licence and an update against. Required to export." }
display_name = { type = "string", default = "", order = 2, help = "The name under the icon. Empty means [application] name." }
version = { type = "string", default = "", order = 3, help = "versionName: what a player is shown. Required to export." }
version_code = { type = "int", default = 0, min = 0, max = 2100000000, order = 4, help = "versionCode: what Play orders updates by, and the only one it reads. Required to export; 0 is unset." }
min_sdk = { type = "int", default = 0, min = 0, max = 40, order = 5, help = "The API floor. 0 defers to the template's own, which is what its libraries were built against." }
target_sdk = { type = "int", default = 36, min = 21, max = 40, order = 6, help = "The API this game says it was written for. Google Play takes a new app or an update only at 36 or later." }
abis = { type = "flags", default = [], options = ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"], order = 7, help = "Which of the template's ABIs the export keeps. None named means every one it carries, so a game that says nothing ships everywhere." }
keystore = { type = "string", default = "", order = 8, help = "A project-relative keystore. Without one an export needs --sign debug, which installs on a device and ships nowhere." }
key = { type = "string", default = "", order = 9, help = "Which key in that keystore signs." }
bundletool = { type = "string", default = "", order = 10, help = "Where bundletool.jar is. Empty looks at BALAUR_ANDROID_BUNDLETOOL and then beside the SDK; Google ships it on its own, not in the SDK." }
permissions = { type = "list", of = { type = "string" }, default = ["INTERNET"], order = 11, help = "The permissions the manifest declares: INTERNET for android.permission.INTERNET, or a whole name. Without INTERNET no socket opens; a game that never goes online writes permissions = []." }
schemes = { type = "list", of = { type = "string" }, default = [], order = 12, help = "The custom URL schemes that open the game, each an intent filter: the scheme a platform.web_sign_in page ends on (com.studio.game for com.studio.game:/done), which its Custom Tab answers with." }
capabilities = { type = "flags", default = [], options = ["play-games", "in-app-purchase", "in-app-review", "firebase-messaging"], order = 13, help = "The Google services the game uses, each shipping its Google libraries: play-games for platform.* on Play Games, in-app-purchase for Play Billing, in-app-review for device.request_review, firebase-messaging for platform.register_for_push. None named ships no Google library." }
games_project_id = { type = "string", default = "", order = 14, help = "The Play Games project id Play Console shows under Play Games Services, all digits. Required with play-games." }
google_services = { type = "string", default = "", order = 15, help = "The project-relative google-services.json the Firebase console hands out, holding this application_id's app. Required with firebase-messaging, whose project, app id, API key and sender id it writes as the string resources Firebase starts from. The file itself stays out of the game's pack." }
notification_icon = { type = "string", default = "", order = 16, help = "A project-relative square picture with transparency, the small icon Android draws in the status bar for the game's notifications, local and pushed. Only its alpha is read: the export writes it white at each density. Empty shows the system's reminder icon." }
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
bundle_id = { type = "string", default = "", order = 1, help = "The identifier registered in App Store Connect. Required to export." }
team_id = { type = "string", default = "", order = 2, help = "The ten-character team identifier. Nothing expands it, so an entitlement carrying the prefix needs the real value." }
display_name = { type = "string", default = "", order = 3, help = "The name under the icon. Empty means [application] name." }
version = { type = "string", default = "", order = 4, help = "CFBundleShortVersionString: what a player is shown. Required to export." }
build_number = { type = "string", default = "", order = 5, help = "CFBundleVersion: what the store orders uploads by. Required to export." }
min_ios = { type = "string", default = "15.0", order = 6, help = "MinimumOSVersion on iOS. A plist may not claim less than the binary was built for." }
min_macos = { type = "string", default = "12.0", order = 7, help = "LSMinimumSystemVersion on macOS." }
category = { type = "string", default = "", order = 8, help = "LSApplicationCategoryType, macOS only, which the Mac App Store requires: e.g. public.app-category.education." }
capabilities = { type = "flags", default = [], options = ["sign-in-with-apple", "game-center", "icloud-kv", "in-app-purchase", "push-notifications"], order = 9, help = "What the bundle declares it uses. Each writes its own entitlement and its own minimum OS. push-notifications writes aps-environment: development for a build signed by a development identity or none, production for a distribution one." }
macos_identity = { type = "string", default = "", order = 10, help = "Developer ID Application: … for a download, Apple Distribution: … for the Mac App Store. The password behind it is read from the environment, never from here." }
macos_installer_identity = { type = "string", default = "", order = 17, help = "The identity a Mac App Store .pkg is signed with, 3rd Party Mac Developer Installer: …. Empty derives it from an Apple Distribution macos_identity." }
macos_provisioning_profile = { type = "string", default = "", order = 11, help = "A project-relative .provisionprofile, the Developer ID or Mac App Store profile macos_identity goes with, embedded as Contents/embedded.provisionprofile. A signed Mac build needs one for sign-in-with-apple, game-center, icloud-kv or push-notifications; a capability the profile does not grant (a Developer ID profile has no Sign in with Apple) is left out of the Mac build, which answers unsupported for it." }
notarize = { type = "bool", default = false, order = 12, help = "Submit to Apple's notary service after signing, and staple the ticket." }
ios_identity = { type = "string", default = "", order = 13, help = "The identity an iOS build is signed with." }
ios_provisioning_profile = { type = "string", default = "", order = 14, help = "A project-relative .mobileprovision, copied into the bundle. The export refuses one that does not grant what the capabilities write." }
schemes = { type = "list", of = { type = "string" }, default = [], order = 15, help = "The custom URL schemes that open the game, as CFBundleURLTypes on iOS and a Mac: a link on one arrives as url_opened on platform_event. Named by [android] schemes' rule: lowercase, a letter first, then letters, digits, +, - or ., and not http or https." }
associated_domains = { type = "list", of = { type = "string" }, default = [], order = 16, help = "com.apple.developer.associated-domains, each as Apple writes it (applinks:game.studio.com): a universal link to one opens the game, arriving as url_opened. Granted by the provisioning profile only, which the export checks, and the domain serves apple-app-site-association naming the app." }
"#,
        ),
    );
    define_group(
        eng,
        words::native::WINDOWS,
        Scope::Project,
        &parse(
            "settings.windows",
            r#"
certificate = { type = "string", default = "", order = 1, help = "A project-relative .pfx, or an Azure Trusted Signing metadata file when the key lives in a cloud HSM." }
timestamp_url = { type = "string", default = "http://timestamp.digicert.com", order = 2, help = "The timestamp authority a Windows signature is countersigned by." }
"#,
        ),
    );
    define_group(
        eng,
        words::steam::STEAM,
        Scope::Project,
        &parse(
            "settings.steam",
            r#"
app_id = { type = "int", default = 0, min = 0, max = 4294967295, order = 1, help = "The app id Steamworks gave the game; 0 is unset. `balaur export --steam`, which needs it, ships Valve's steam_api beside the game; with --dev it also writes the id to steam_appid.txt, so the game starts without the Steam client. An export without --steam ships no Valve file." }
"#,
        ),
    );
}
