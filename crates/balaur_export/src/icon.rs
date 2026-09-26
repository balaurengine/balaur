//! App icons: the picture `[application] icon` names, written the way each
//! platform reads one. `icon_dark` and `icon_monochrome` add the forms iOS,
//! macOS, Android and a browser show in a dark or a tinted theme.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use image::imageops::FilterType;
use image::{ImageEncoder, RgbaImage};

/// The size every icon is prepared at: what an App Store and a Play listing
/// ask for, and a whole multiple of every size written from it.
pub(crate) const SIDE: u32 = 1024;

/// The keys, in `[application]`.
pub(crate) const ICON: &str = "icon";
pub(crate) const ICON_DARK: &str = "icon_dark";
pub(crate) const ICON_MONOCHROME: &str = "icon_monochrome";

/// An adaptive icon's layers are 108 dp across, and a launcher's mask keeps
/// the middle 66: the picture is inset to that, so no shape crops it.
const SAFE_ZONE: f32 = 66.0 / 108.0;

/// The project's icon, and the forms beside it, each square at [`SIDE`].
pub(crate) struct Icons {
    plain: RgbaImage,
    dark: Option<RgbaImage>,
    monochrome: Option<RgbaImage>,
    /// `[application] name`, which a desktop entry and a web manifest show.
    title: Option<String>,
}

impl Icons {
    /// The icons `[application]` names, or `None` for a project with none.
    ///
    /// # Errors
    /// A file that will not read, or a picture that is not square.
    pub(crate) fn load(project: &Path, manifest: &toml::Table) -> Result<Option<Self>> {
        let named = |key: &str| {
            manifest
                .get("application")
                .and_then(|a| a.get(key))
                .and_then(toml::Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .map(str::to_string)
        };
        let Some(plain) = named(ICON) else {
            return Ok(None);
        };
        let title = named("name");
        let read = |key: &str, path: &str| {
            picture(project, path).with_context(|| format!("[application] {key} = \"{path}\""))
        };
        Ok(Some(Self {
            plain: read(ICON, &plain)?,
            dark: named(ICON_DARK).map(|p| read(ICON_DARK, &p)).transpose()?,
            monochrome: named(ICON_MONOCHROME)
                .map(|p| read(ICON_MONOCHROME, &p))
                .transpose()?,
            title,
        }))
    }

    /// A set from pictures already in hand, for a test.
    #[cfg(test)]
    pub(crate) fn of(
        plain: RgbaImage,
        dark: Option<RgbaImage>,
        monochrome: Option<RgbaImage>,
    ) -> Self {
        Self {
            plain,
            dark,
            monochrome,
            title: None,
        }
    }

    /// The game's name as a launcher shows it: `[application] name`, or the
    /// exporter's own when the project names none.
    fn title<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.title.as_deref().unwrap_or(fallback)
    }

    /// The Linux pair beside the executable: the picture, and the desktop entry
    /// a packager installs with it into `share/applications`.
    pub(crate) fn write_linux(&self, executable: &Path, fallback: &str) -> Result<()> {
        let display = self.title(fallback);
        let stem = executable
            .file_name()
            .map_or_else(|| "game".into(), |n| n.to_string_lossy().into_owned());
        let dir = executable.parent().unwrap_or_else(|| Path::new("."));
        write(&dir.join(format!("{stem}.png")), &png(&self.plain, 512))?;
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={display}\nExec={stem}\nIcon={stem}\nTerminal=false\nCategories=Game;\n"
        );
        write(&dir.join(format!("{stem}.desktop")), entry.as_bytes())
    }

    /// A Windows executable with the icon in its resources, where Explorer and
    /// the taskbar read it. Written before the pack is fused and the file is
    /// signed, since both come after the image this rewrites.
    pub(crate) fn exe_with_icon(&self, runtime: &[u8]) -> Result<Vec<u8>> {
        let mut image =
            editpe::Image::parse(runtime).context("reading the runtime as a PE image")?;
        let mut resources = image.resource_directory().cloned().unwrap_or_default();
        resources
            .set_main_icon(self.ico())
            .context("writing the icon into the runtime's resources")?;
        image
            .set_resource_directory(resources)
            .context("writing the runtime's resources back")?;
        let mut out = Vec::with_capacity(runtime.len() + 256 * 1024);
        image
            .write_writer(&mut out)
            .context("writing the runtime with its icon")?;
        Ok(out)
    }

    /// An `.ico` of the sizes Windows asks for, each a PNG.
    pub(crate) fn ico(&self) -> Vec<u8> {
        const SIZES: [u32; 6] = [256, 128, 48, 32, 24, 16];
        let pictures: Vec<Vec<u8>> = SIZES.iter().map(|s| png(&self.plain, *s)).collect();
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * SIZES.len() as u32;
        for (size, picture) in SIZES.iter().zip(&pictures) {
            // 256 is written as 0: the byte cannot hold it.
            let side = if *size >= 256 { 0 } else { *size as u8 };
            out.extend_from_slice(&[side, side, 0, 0]);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&(picture.len() as u32).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += picture.len() as u32;
        }
        for picture in &pictures {
            out.extend_from_slice(picture);
        }
        out
    }

    /// An `.icns` of the sizes a Mac draws, each a PNG under its OSType.
    pub(crate) fn icns(&self) -> Vec<u8> {
        const ENTRIES: [(&[u8; 4], u32); 8] = [
            (b"ic11", 32),
            (b"ic12", 64),
            (b"ic07", 128),
            (b"ic13", 256),
            (b"ic08", 256),
            (b"ic14", 512),
            (b"ic09", 512),
            (b"ic10", 1024),
        ];
        let mut body = Vec::new();
        for (kind, size) in ENTRIES {
            let picture = png(&self.plain, size);
            body.extend_from_slice(kind);
            body.extend_from_slice(&(picture.len() as u32 + 8).to_be_bytes());
            body.extend_from_slice(&picture);
        }
        let mut out = Vec::with_capacity(body.len() + 8);
        out.extend_from_slice(b"icns");
        out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// The files a web export writes beside its page: the favicons, and the
    /// manifest an installed page reads its icons and its name from.
    pub(crate) fn web_files(&self, fallback: &str) -> Vec<(String, Vec<u8>)> {
        let name = self.title(fallback);
        let mut out = vec![
            ("icon.png".to_string(), png(&self.plain, 512)),
            ("icon-192.png".to_string(), png(&self.plain, 192)),
            ("apple-touch-icon.png".to_string(), png(&self.plain, 180)),
        ];
        let mut icons = vec![
            r#"{ "src": "icon-192.png", "sizes": "192x192", "type": "image/png" }"#.to_string(),
            r#"{ "src": "icon.png", "sizes": "512x512", "type": "image/png" }"#.to_string(),
        ];
        if let Some(dark) = &self.dark {
            out.push(("icon-dark.png".to_string(), png(dark, 512)));
        }
        if let Some(monochrome) = &self.monochrome {
            out.push(("icon-monochrome.png".to_string(), png(monochrome, 512)));
            icons.push(
                r#"{ "src": "icon-monochrome.png", "sizes": "512x512", "type": "image/png", "purpose": "monochrome" }"#
                    .to_string(),
            );
        }
        let manifest = format!(
            "{{\n  \"name\": {},\n  \"display\": \"fullscreen\",\n  \"icons\": [\n    {}\n  ]\n}}\n",
            json_string(name),
            icons.join(",\n    ")
        );
        out.push((WEB_MANIFEST.to_string(), manifest.into_bytes()));
        out
    }

    /// The `<link>` lines naming [`Self::web_files`], for the page's head.
    pub(crate) fn web_links(&self) -> String {
        let mut out = format!(
            "<link rel=\"icon\" href=\"icon.png\">\n<link rel=\"apple-touch-icon\" href=\"apple-touch-icon.png\">\n<link rel=\"manifest\" href=\"{WEB_MANIFEST}\">\n"
        );
        if self.dark.is_some() {
            out.push_str(
                "<link rel=\"icon\" href=\"icon-dark.png\" media=\"(prefers-color-scheme: dark)\">\n",
            );
        }
        out
    }

    /// The icon inside a macOS `.app`: an `.icns` every macOS reads, and on a
    /// Mac with Xcode an asset catalog carrying the dark and tinted forms.
    /// Answers the `Info.plist` keys that name them.
    pub(crate) fn write_macos(&self, app: &Path, minimum: &str) -> Result<String> {
        let resources = app.join("Contents").join("Resources");
        write(&resources.join("AppIcon.icns"), &self.icns())?;
        let catalog = self
            .compile_catalog(&resources, app, Catalog::Mac, minimum)
            .unwrap_or_default();
        // actool writes its own AppIcon.icns and names it; a key twice is a
        // plist codesign may reject.
        let mut keys = String::new();
        if !catalog.contains("CFBundleIconFile") {
            plist_string(&mut keys, "CFBundleIconFile", "AppIcon");
        }
        keys.push_str(&catalog);
        Ok(keys)
    }

    /// The icon inside an iOS `.app`. On a Mac with Xcode it is an asset
    /// catalog with the dark and tinted forms, which is what the App Store
    /// takes; elsewhere it is the loose files a device install reads.
    /// Answers the `Info.plist` keys that name them.
    pub(crate) fn write_ios(&self, app: &Path, minimum: &str) -> Result<String> {
        if let Some(catalog) = self.compile_catalog(app, app, Catalog::Ios, minimum) {
            return Ok(catalog);
        }
        for (file, size) in [
            ("AppIcon60x60@2x.png", 120),
            ("AppIcon60x60@3x.png", 180),
            ("AppIcon76x76@2x~ipad.png", 152),
            ("AppIcon83.5x83.5@2x~ipad.png", 167),
        ] {
            write(&app.join(file), &png(&self.plain, size))?;
        }
        tracing::info!(
            "iOS icon written as loose files, which a device install shows; the App Store takes one \
             only from an asset catalog, which an export on a Mac with Xcode compiles"
        );
        Ok(concat!(
            "  <key>CFBundleIcons</key><dict><key>CFBundlePrimaryIcon</key><dict>",
            "<key>CFBundleIconFiles</key><array><string>AppIcon60x60</string></array></dict></dict>\n",
            "  <key>CFBundleIcons~ipad</key><dict><key>CFBundlePrimaryIcon</key><dict>",
            "<key>CFBundleIconFiles</key><array><string>AppIcon60x60</string>",
            "<string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array></dict></dict>\n",
        )
        .to_string())
    }

    /// Compile an `AppIcon` asset catalog into `into` with Xcode's `actool`,
    /// and answer the plist keys it asks for. `None` where there is no
    /// `actool`, or it refused the catalog: the loose files stand in. The
    /// catalog is staged beside `bundle`, outside what codesign seals.
    fn compile_catalog(
        &self,
        into: &Path,
        bundle: &Path,
        catalog: Catalog,
        minimum: &str,
    ) -> Option<String> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let work = tempdir_in(bundle)?;
        let written = self.write_catalog(&work, catalog);
        let answer = written.and_then(|assets| {
            let partial = work.join("partial.plist");
            let mut command = Command::new("xcrun");
            command
                .arg("actool")
                .arg(&assets)
                .arg("--compile")
                .arg(into)
                .args(["--platform", catalog.platform()])
                .args(["--minimum-deployment-target", minimum])
                .args(["--app-icon", "AppIcon"])
                .arg("--output-partial-info-plist")
                .arg(&partial)
                .args(["--output-format", "human-readable-text"]);
            for device in catalog.devices() {
                command.args(["--target-device", device]);
            }
            let ran = command.output().context("running xcrun actool")?;
            if !ran.status.success() {
                bail!("actool: {}", String::from_utf8_lossy(&ran.stdout).trim());
            }
            let text =
                std::fs::read_to_string(&partial).context("reading actool's partial plist")?;
            Ok(plist_body(&text))
        });
        let _ = std::fs::remove_dir_all(&work);
        match answer {
            Ok(keys) => Some(keys),
            Err(why) => {
                tracing::info!("no asset catalog, so no dark or tinted icon: {why:#}");
                None
            }
        }
    }

    /// `Assets.xcassets/AppIcon.appiconset`, with the dark and tinted
    /// appearances the set carries.
    fn write_catalog(&self, work: &Path, catalog: Catalog) -> Result<PathBuf> {
        let assets = work.join("Assets.xcassets");
        let set = assets.join("AppIcon.appiconset");
        std::fs::create_dir_all(&set)?;
        write(
            &assets.join("Contents.json"),
            br#"{ "info" : { "author" : "balaur", "version" : 1 } }"#,
        )?;
        let forms: Vec<(&str, &RgbaImage, &str)> = [
            Some(("", &self.plain, "")),
            self.dark.as_ref().map(|d| ("-dark", d, "dark")),
            self.monochrome.as_ref().map(|m| ("-tinted", m, "tinted")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut images = Vec::new();
        for (suffix, picture, appearance) in forms {
            let look = if appearance.is_empty() {
                String::new()
            } else {
                format!(
                    r#""appearances" : [ {{ "appearance" : "luminosity", "value" : "{appearance}" }} ], "#
                )
            };
            match catalog {
                Catalog::Ios => {
                    let file = format!("icon{suffix}.png");
                    write(&set.join(&file), &png(picture, SIDE))?;
                    images.push(format!(
                        r#"{{ {look}"filename" : "{file}", "idiom" : "universal", "platform" : "ios", "size" : "1024x1024" }}"#
                    ));
                }
                Catalog::Mac => {
                    for (points, scale) in [
                        (16, 1),
                        (16, 2),
                        (32, 1),
                        (32, 2),
                        (128, 1),
                        (128, 2),
                        (256, 1),
                        (256, 2),
                        (512, 1),
                        (512, 2),
                    ] {
                        let file = format!("icon{suffix}-{points}@{scale}x.png");
                        write(&set.join(&file), &png(picture, points * scale))?;
                        images.push(format!(
                            r#"{{ {look}"filename" : "{file}", "idiom" : "mac", "scale" : "{scale}x", "size" : "{points}x{points}" }}"#
                        ));
                    }
                }
            }
        }
        let contents = format!(
            "{{\n  \"images\" : [\n    {}\n  ],\n  \"info\" : {{ \"author\" : \"balaur\", \"version\" : 1 }}\n}}\n",
            images.join(",\n    ")
        );
        write(&set.join("Contents.json"), contents.as_bytes())?;
        Ok(assets)
    }

    /// The resources an Android layout gains: the icon at each density, and
    /// the adaptive icon over it that Android 8 and on draws, with the
    /// monochrome layer Android 13 tints when a project names one. The
    /// adaptive background is the picture's corner when that is opaque, so a
    /// square icon's own ground fills the mask, and white otherwise.
    pub(crate) fn write_android(&self, layout: &Path) -> Result<()> {
        let res = layout.join("res");
        for (density, legacy) in [
            ("mdpi", 48),
            ("hdpi", 72),
            ("xhdpi", 96),
            ("xxhdpi", 144),
            ("xxxhdpi", 192),
        ] {
            let dir = res.join(format!("mipmap-{density}"));
            write(&dir.join("icon.png"), &png(&self.plain, legacy))?;
            let layer = legacy * 9 / 4;
            write(
                &dir.join("icon_foreground.png"),
                &inset_png(&self.plain, layer),
            )?;
            if let Some(monochrome) = &self.monochrome {
                write(
                    &dir.join("icon_monochrome.png"),
                    &inset_png(monochrome, layer),
                )?;
            }
        }
        let corner = self.plain.get_pixel(0, 0).0;
        let ground = if corner[3] == 255 {
            format!("#{:02x}{:02x}{:02x}", corner[0], corner[1], corner[2])
        } else {
            "#ffffff".to_string()
        };
        let colors = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n    <color name=\"icon_background\">{ground}</color>\n</resources>\n"
        );
        write(
            &res.join("values").join("icon_background.xml"),
            colors.as_bytes(),
        )?;
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\">\n    <background android:drawable=\"@color/icon_background\"/>\n    <foreground android:drawable=\"@mipmap/icon_foreground\"/>\n",
        );
        if self.monochrome.is_some() {
            xml.push_str("    <monochrome android:drawable=\"@mipmap/icon_monochrome\"/>\n");
        }
        xml.push_str("</adaptive-icon>\n");
        write(
            &res.join("mipmap-anydpi-v26").join("icon.xml"),
            xml.as_bytes(),
        )
    }
}

/// The resource an Android manifest names its icon by.
pub(crate) const ANDROID_ICON: &str = "@mipmap/icon";

/// The web app manifest a web export writes beside its page.
const WEB_MANIFEST: &str = "manifest.webmanifest";

/// Where a page's icon links go when its shell names no `{{icons}}`.
const HEAD_END: &str = "</head>";

/// A web shell with its placeholders filled: `{{title}}`, `{{pack}}`, and
/// `{{icons}}`, which a project's own page may leave out, in which case the
/// links go before its `</head>`.
pub(crate) fn web_page(shell: &str, title: &str, pack: &str, icons: Option<&Icons>) -> String {
    let links = icons.map(Icons::web_links).unwrap_or_default();
    let page = shell.replace("{{title}}", title).replace("{{pack}}", pack);
    if page.contains("{{icons}}") {
        return page.replace("{{icons}}", links.trim_end());
    }
    match page.find(HEAD_END) {
        Some(at) if !links.is_empty() => format!("{}{links}{}", &page[..at], &page[at..]),
        _ => page,
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy)]
enum Catalog {
    Ios,
    Mac,
}

impl Catalog {
    const fn platform(self) -> &'static str {
        match self {
            Self::Ios => "iphoneos",
            Self::Mac => "macosx",
        }
    }

    const fn devices(self) -> &'static [&'static str] {
        match self {
            Self::Ios => &["iphone", "ipad"],
            Self::Mac => &["mac"],
        }
    }
}

/// A picture from the project, square, at [`SIDE`]. An SVG is drawn at that
/// size rather than scaled from a smaller one.
fn picture(project: &Path, path: &str) -> Result<RgbaImage> {
    let bytes = balaur::files::default_backend()
        .read(&project.join(path))
        .with_context(|| format!("reading {path}"))?;
    let (width, height) = balaur::pixels::size(&bytes, &toml::Table::new())?;
    if width != height {
        bail!("an icon is square, and {path} is {width} by {height}");
    }
    let image = if balaur::pixels::is_svg(&bytes) {
        balaur::pixels::rasterize_svg(&bytes, SIDE as f32 / width.max(1) as f32)?
    } else {
        image::load_from_memory(&bytes)?.to_rgba8()
    };
    Ok(if image.width() == SIDE {
        image
    } else {
        image::imageops::resize(&image, SIDE, SIDE, FilterType::Lanczos3)
    })
}

/// The picture at `size`, as a PNG.
fn png(picture: &RgbaImage, size: u32) -> Vec<u8> {
    let scaled;
    let at = if picture.width() == size {
        picture
    } else {
        scaled = image::imageops::resize(picture, size, size, FilterType::Lanczos3);
        &scaled
    };
    encode(at)
}

/// The picture inset to an adaptive layer's safe zone, on transparency.
fn inset_png(picture: &RgbaImage, size: u32) -> Vec<u8> {
    let inner = (size as f32 * SAFE_ZONE).round() as u32;
    let scaled = image::imageops::resize(picture, inner, inner, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(size, size);
    let at = i64::from((size - inner) / 2);
    image::imageops::overlay(&mut canvas, &scaled, at, at);
    encode(&canvas)
}

fn encode(picture: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    // Writing to a Vec fails only if the image and its size disagree, which
    // they cannot: both come from the same buffer.
    let _ = image::codecs::png::PngEncoder::new(&mut out).write_image(
        picture,
        picture.width(),
        picture.height(),
        image::ExtendedColorType::Rgba8,
    );
    out
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn plist_string(keys: &mut String, key: &str, value: &str) {
    let _ = writeln!(keys, "  <key>{key}</key><string>{value}</string>");
}

/// What sits inside a plist's top-level `<dict>`: `actool` writes a whole
/// document, and the keys join the one the exporter writes.
fn plist_body(text: &str) -> String {
    let start = text.find("<dict>").map_or(0, |at| at + "<dict>".len());
    let end = text.rfind("</dict>").unwrap_or(text.len());
    text.get(start..end).unwrap_or_default().to_string()
}

/// A scratch directory beside the bundle, so `actool` writes on the same disk.
fn tempdir_in(beside: &Path) -> Option<PathBuf> {
    let dir = beside.with_extension("icon-staging");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(SIDE, SIDE, image::Rgba(rgba))
    }

    fn icons() -> Icons {
        Icons::of(
            square([200, 40, 40, 255]),
            Some(square([20, 20, 60, 255])),
            Some(square([255, 255, 255, 255])),
        )
    }

    /// Every entry an `.ico` lists points at a PNG of the size it claims.
    #[test]
    fn an_ico_lists_six_sizes_each_a_png_of_its_size() {
        let ico = icons().ico();
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, 6);
        for i in 0..count {
            let entry = &ico[6 + i * 16..6 + i * 16 + 16];
            let side = if entry[0] == 0 {
                256
            } else {
                u32::from(entry[0])
            };
            let len = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let at = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            let picture = image::load_from_memory(&ico[at..at + len]).unwrap();
            assert_eq!((picture.width(), picture.height()), (side, side));
        }
    }

    /// An `.icns` is its length, then one PNG per OSType, each as long as it says.
    #[test]
    fn an_icns_carries_every_size_a_mac_draws() {
        let icns = icons().icns();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
            icns.len()
        );
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < icns.len() {
            let len = u32::from_be_bytes(icns[at + 4..at + 8].try_into().unwrap()) as usize;
            let picture = image::load_from_memory(&icns[at + 8..at + len]).unwrap();
            kinds.push((
                String::from_utf8_lossy(&icns[at..at + 4]).into_owned(),
                picture.width(),
            ));
            at += len;
        }
        assert!(kinds.contains(&("ic10".into(), 1024)));
        assert!(kinds.contains(&("ic11".into(), 32)));
        assert_eq!(kinds.len(), 8);
    }

    #[test]
    fn a_web_export_links_a_dark_favicon_only_when_there_is_one() {
        let both = icons();
        assert!(both.web_links().contains("prefers-color-scheme: dark"));
        assert!(
            both.web_files("Tide")
                .iter()
                .any(|(name, _)| name == "icon-dark.png")
        );
        let plain = Icons::of(square([0, 0, 0, 255]), None, None);
        assert!(!plain.web_links().contains("dark"));
        let names: Vec<String> = plain
            .web_files("Tide")
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(
            names,
            [
                "icon.png",
                "icon-192.png",
                "apple-touch-icon.png",
                "manifest.webmanifest"
            ]
        );
    }

    /// The manifest is what an installed page reads its name and icons from,
    /// and the monochrome form is listed under its own purpose.
    #[test]
    fn the_web_manifest_names_the_game_and_its_monochrome_icon() {
        let files = icons().web_files("Say \"hi\"");
        let (_, manifest) = files.iter().find(|(n, _)| n == WEB_MANIFEST).unwrap();
        let text = std::str::from_utf8(manifest).unwrap();
        assert!(text.contains(r#""name": "Say \"hi\"""#), "{text}");
        assert!(text.contains(r#""purpose": "monochrome""#), "{text}");
    }

    #[test]
    fn a_page_without_an_icons_placeholder_gets_the_links_before_its_head_closes() {
        let own = "<html><head><title>{{title}}</title></head><body>{{pack}}</body></html>";
        let page = web_page(own, "Tide", "game.bpak", Some(&icons()));
        let links = page.find("rel=\"icon\"").unwrap();
        assert!(links < page.find("</head>").unwrap(), "{page}");
        assert!(page.contains("<title>Tide</title>") && page.contains("game.bpak"));
        let bare = web_page(own, "Tide", "game.bpak", None);
        assert!(!bare.contains("rel=\"icon\""));
    }

    #[test]
    fn an_opaque_icon_fills_the_adaptive_background_with_its_own_corner() {
        let dir = tempfile::tempdir().unwrap();
        icons().write_android(dir.path()).unwrap();
        let colors =
            std::fs::read_to_string(dir.path().join("res/values/icon_background.xml")).unwrap();
        assert!(colors.contains("#c82828"), "{colors}");
    }

    /// The adaptive layers are 108 dp and the picture sits inside the 66 in
    /// their middle: a corner of the layer is empty, its centre is not.
    #[test]
    fn an_android_layout_gets_every_density_and_an_adaptive_icon() {
        let dir = tempfile::tempdir().unwrap();
        icons().write_android(dir.path()).unwrap();
        let res = dir.path().join("res");
        let legacy = image::open(res.join("mipmap-xxxhdpi/icon.png")).unwrap();
        assert_eq!(legacy.width(), 192);
        let layer = image::open(res.join("mipmap-xxxhdpi/icon_foreground.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(layer.width(), 432);
        assert_eq!(
            layer.get_pixel(0, 0).0[3],
            0,
            "the corner is outside the safe zone"
        );
        assert_eq!(layer.get_pixel(216, 216).0[3], 255);
        let xml = std::fs::read_to_string(res.join("mipmap-anydpi-v26/icon.xml")).unwrap();
        assert!(xml.contains("@mipmap/icon_foreground") && xml.contains("<monochrome"));
    }

    #[test]
    fn a_linux_export_writes_the_picture_and_a_desktop_entry_beside_the_game() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("pirates");
        icons().write_linux(&game, "Polyglot Pirates").unwrap();
        let entry = std::fs::read_to_string(dir.path().join("pirates.desktop")).unwrap();
        assert!(entry.contains("Name=Polyglot Pirates") && entry.contains("Icon=pirates"));
        assert_eq!(
            image::open(dir.path().join("pirates.png")).unwrap().width(),
            512
        );
    }

    /// Off a Mac there is no `actool`, so an iOS bundle gets the loose files
    /// and the plist keys that name them.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn an_ios_bundle_off_a_mac_gets_loose_icons_and_their_keys() {
        let dir = tempfile::tempdir().unwrap();
        let keys = icons().write_ios(dir.path(), "15.0").unwrap();
        assert!(keys.contains("CFBundleIconFiles") && keys.contains("AppIcon60x60"));
        assert_eq!(
            image::open(dir.path().join("AppIcon60x60@3x.png"))
                .unwrap()
                .width(),
            180
        );
    }

    #[test]
    fn a_catalog_carries_the_dark_and_tinted_appearances() {
        let dir = tempfile::tempdir().unwrap();
        let assets = icons().write_catalog(dir.path(), Catalog::Ios).unwrap();
        let contents =
            std::fs::read_to_string(assets.join("AppIcon.appiconset/Contents.json")).unwrap();
        assert!(
            contents.contains(r#""value" : "dark""#) && contents.contains(r#""value" : "tinted""#)
        );
        assert!(assets.join("AppIcon.appiconset/icon-dark.png").exists());
    }

    /// The smallest PE32+ a loader takes: headers and one `.text` section
    /// holding a `ret`, and no resources, as a Rust runtime is linked.
    fn bare_exe() -> Vec<u8> {
        let mut pe = vec![0u8; 0x400];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        let mut at = 0x40;
        let mut put = |bytes: &[u8]| {
            pe[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(b"PE\0\0");
        // COFF: x86-64, one section, a PE32+ optional header, an executable image.
        put(&0x8664u16.to_le_bytes());
        put(&1u16.to_le_bytes());
        put(&[0; 12]);
        put(&240u16.to_le_bytes());
        put(&0x22u16.to_le_bytes());
        put(&0x20bu16.to_le_bytes());
        put(&[0; 2]);
        for field in [0x200u32, 0, 0, 0x1000, 0x1000] {
            put(&field.to_le_bytes());
        }
        put(&0x1_4000_0000u64.to_le_bytes());
        put(&0x1000u32.to_le_bytes());
        put(&0x200u32.to_le_bytes());
        for field in [6u16, 0, 0, 0, 6, 0] {
            put(&field.to_le_bytes());
        }
        for field in [0u32, 0x2000, 0x200, 0] {
            put(&field.to_le_bytes());
        }
        put(&3u16.to_le_bytes());
        put(&0x8160u16.to_le_bytes());
        for field in [0x10_0000u64, 0x1000, 0x10_0000, 0x1000] {
            put(&field.to_le_bytes());
        }
        put(&0u32.to_le_bytes());
        put(&16u32.to_le_bytes());
        put(&[0; 128]);
        put(b".text\0\0\0");
        for field in [0x10u32, 0x1000, 0x200, 0x200, 0, 0] {
            put(&field.to_le_bytes());
        }
        put(&[0; 4]);
        put(&0x6000_0020u32.to_le_bytes());
        pe[0x200] = 0xc3;
        pe
    }

    #[test]
    fn a_windows_runtime_gains_the_icon_in_its_resources() {
        let exe = icons().exe_with_icon(&bare_exe()).unwrap();
        let image = editpe::Image::parse(exe.as_slice()).unwrap();
        let first = image
            .resource_directory()
            .and_then(|r| r.get_main_icon().ok().flatten())
            .expect("an icon group in the resources");
        assert_eq!(image::load_from_memory(first).unwrap().width(), 256);
    }

    #[test]
    fn a_picture_that_is_not_square_is_refused_with_its_size() {
        let dir = tempfile::tempdir().unwrap();
        RgbaImage::new(300, 200)
            .save(dir.path().join("wide.png"))
            .unwrap();
        let manifest: toml::Table = toml::from_str("[application]\nicon = \"wide.png\"").unwrap();
        let why = Icons::load(dir.path(), &manifest).err().unwrap();
        assert!(format!("{why:#}").contains("300 by 200"), "{why:#}");
    }
}
