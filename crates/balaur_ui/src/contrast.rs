//! WCAG contrast over a `widget_theme` document: what each role's ink reads at
//! on the fill it is drawn on. The editor's theme window marks a pair from
//! it, and the test on the bundled themes asserts none falls short.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::theme::parse_hex;
use crate::vocabulary::{keys as k, tokens, weights};

/// The ratio body text needs, and the one large or bold text needs.
pub const AA: f64 = 4.5;
pub const AA_LARGE: f64 = 3.0;

/// One ink on one fill, as a role draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    /// The role, and the state table under it when there is one: `tab.hover`.
    pub role: String,
    /// The token or `#rrggbb` the role names for its text, and for its fill.
    pub ink: String,
    pub fill: String,
    pub ratio: f64,
    /// What the role's size asks for: [`AA_LARGE`] from 18 px, or 14 px bold.
    pub need: f64,
}

/// An sRGB channel as linear light, 0 to 1.
pub(crate) fn to_linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        libm::pow((c + 0.055) / 1.055, 2.4)
    }
}

fn luminance(color: egui::Color32) -> f64 {
    0.2126 * to_linear(color.r()) + 0.7152 * to_linear(color.g()) + 0.0722 * to_linear(color.b())
}

/// The WCAG 2 contrast ratio between two colours, 1 to 21.
#[must_use]
pub fn ratio(a: egui::Color32, b: egui::Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// A token looked up in `[colors]`, or a literal colour, as a colour.
#[must_use]
pub fn color_of(colors: Option<&toml::Table>, value: &str) -> Option<egui::Color32> {
    let hex = colors
        .and_then(|colors| colors.get(value))
        .and_then(toml::Value::as_str)
        .unwrap_or(value);
    crate::theme::parse_hex(hex)
}

/// Every ink a document's roles draw, on the fill each draws it on, with the
/// tokens it leaves out derived from its sources.
///
/// A state table inherits the ink, fill and size of the table above it. A role
/// that states no fill is drawn on `ground`, the token for the sheet under it.
/// A pair whose ink or fill names no colour is left out.
#[must_use]
pub fn pairs(doc: &toml::Value, ground: &str) -> Vec<Pair> {
    let doc = crate::palette::complete(doc);
    let tokens = Tokens {
        colors: doc.get(k::COLORS).and_then(toml::Value::as_table),
        sizes: doc.get(k::SIZES).and_then(toml::Value::as_table),
    };
    let mut out = Vec::new();
    let Some(roles) = doc.get(k::ROLES).and_then(toml::Value::as_table) else {
        return out;
    };
    for (name, role) in roles {
        if let Some(role) = role.as_table() {
            let base = Inherited {
                fill: ground.to_owned(),
                ink: String::new(),
                size: None,
                strong: false,
            };
            walk(name, role, &base, &tokens, &mut out);
        }
    }
    out
}

struct Tokens<'a> {
    colors: Option<&'a toml::Table>,
    sizes: Option<&'a toml::Table>,
}

impl Tokens<'_> {
    /// A number, or the name of one of the theme's sizes.
    fn number(&self, value: &toml::Value) -> Option<f64> {
        balaur_core::components::as_f64(value).or_else(|| {
            let named = self.sizes?.get(value.as_str()?)?;
            balaur_core::components::as_f64(named)
        })
    }
}

struct Inherited {
    fill: String,
    ink: String,
    size: Option<f64>,
    strong: bool,
}

fn walk(
    name: &str,
    table: &toml::Table,
    above: &Inherited,
    tokens: &Tokens<'_>,
    out: &mut Vec<Pair>,
) {
    let text = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    let here = Inherited {
        fill: text(k::FILL).unwrap_or_else(|| above.fill.clone()),
        ink: text(k::TEXT_COLOR).unwrap_or_else(|| above.ink.clone()),
        size: table
            .get(k::FONT_SIZE)
            .and_then(|size| tokens.number(size))
            .or(above.size),
        strong: table
            .get(k::FONT_WEIGHT)
            .and_then(balaur_core::components::as_f64)
            .map_or(above.strong, |weight| {
                weight >= f64::from(weights::BOLD_FROM)
            }),
    };
    let large = here
        .size
        .is_some_and(|size| size >= 18.0 || (size >= 14.0 && here.strong));
    let colors = tokens.colors;
    if let (Some(ink), Some(fill)) = (color_of(colors, &here.ink), color_of(colors, &here.fill)) {
        out.push(Pair {
            role: name.to_owned(),
            ink: here.ink.clone(),
            fill: here.fill.clone(),
            ratio: ratio(ink, fill),
            need: if large { AA_LARGE } else { AA },
        });
    }
    for (key, value) in table {
        if let Some(state) = value.as_table() {
            walk(&format!("{name}.{key}"), state, &here, tokens, out);
        }
    }
}

/// How far apart two colours stand in CIELAB below which a reader no longer
/// tells them apart at a glance.
pub const DISTINCT: f64 = 10.0;

/// The colour tokens that say something by their hue alone.
const SAYING: &[&str] = &[
    tokens::PRIMARY,
    tokens::SUCCESS,
    tokens::WARNING,
    tokens::DANGER,
];

/// The ways colour vision is simulated, each a matrix over linear sRGB
/// (Machado, Oliveira and Fernandes 2009, full severity); the first is none.
const VISIONS: [(&str, [[f64; 3]; 3]); 4] = [
    (
        "typical",
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    ),
    (
        "protanopia",
        [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
    ),
    (
        "deuteranopia",
        [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
    ),
    (
        "tritanopia",
        [
            [1.255_528, -0.076_749, -0.178_779],
            [-0.078_411, 0.930_809, 0.147_602],
            [0.004_733, 0.691_367, 0.303_900],
        ],
    ),
];

/// Two of a theme's saying colours, how far apart they stand for the reader
/// who sees them closest, and which vision that is.
#[derive(Clone, Debug, PartialEq)]
pub struct Apart {
    pub first: String,
    pub second: String,
    pub distance: f64,
    pub vision: &'static str,
}

/// Every pair of a document's saying colours, `primary`, `success`,
/// `warning` and `danger` with the tokens it leaves out derived, closest
/// first: one under [`DISTINCT`] reads as the same colour to someone.
#[must_use]
pub fn apart(doc: &toml::Value) -> Vec<Apart> {
    let doc = crate::palette::complete(doc);
    let colors = doc.get(k::COLORS).and_then(toml::Value::as_table);
    let held: Vec<(&str, egui::Color32)> = SAYING
        .iter()
        .filter_map(|name| color_of(colors, name).map(|color| (*name, color)))
        .collect();
    let mut out = Vec::new();
    for (at, (first, a)) in held.iter().enumerate() {
        for (second, b) in &held[at + 1..] {
            let (distance, vision) = VISIONS
                .iter()
                .map(|(vision, matrix)| (lab_distance(seen(*a, matrix), seen(*b, matrix)), *vision))
                .fold((f64::INFINITY, VISIONS[0].0), |closest, one| {
                    if one.0 < closest.0 { one } else { closest }
                });
            out.push(Apart {
                first: (*first).to_owned(),
                second: (*second).to_owned(),
                distance,
                vision,
            });
        }
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out
}

/// A colour as a vision sees it, in linear sRGB.
fn seen(color: egui::Color32, matrix: &[[f64; 3]; 3]) -> [f64; 3] {
    let rgb = [color.r(), color.g(), color.b()].map(to_linear);
    matrix.map(|row| (row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]).clamp(0.0, 1.0))
}

/// The CIE76 distance between two linear sRGB colours, through CIELAB under
/// D65.
fn lab_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (a, b) = (lab(a), lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn lab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.950_47;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.088_83;
    let f = |t: f64| {
        if t > 0.008_856 {
            libm::cbrt(t)
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// `ui.contrast` and `ui.contrast_pairs`: the same numbers, for a script.
pub(crate) fn install(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "contrast",
            &[],
            "(a: string, b: string) -> float",
            "The WCAG contrast ratio between two `#rrggbb` colours, 1 to 21. Body text wants 4.5, large or bold text 3.",
        ),
        (
            "contrast_pairs",
            &[],
            "(theme: table, ground: string) -> [ContrastPair]",
            "Every ink a `widget_theme` document's roles draw on the fill each draws it on, as `{ role, ink, fill, ratio, need }`. A role with no fill is read on the `ground` token; `need` is 3 for large or bold text and 4.5 otherwise.",
        ),
    ]);
    m.describe(&[(
        "colors_apart",
        &[],
        "(theme: table) -> [ColorsApart]",
        "Every pair of a `widget_theme` document's colours that say something by hue (`primary`, `success`, `warning`, `danger`), closest first, as `{ first, second, distance, vision }`: how far apart they stand in CIELAB for the colour vision that sees them closest. Under 10 the two read as one colour to that reader.",
    )]);
    m.records(&[(
        "ColorsApart",
        "{ first: string, second: string, distance: float, vision: string }",
        "Two colours a theme says something with, and how far apart they stand for the reader who sees them closest.",
    )]);
    m.function("colors_apart", |_: &Engine, doc: Value| {
        let doc = balaur_core::node_api::to_toml(&doc)?;
        Ok(Value::List(
            apart(&doc)
                .into_iter()
                .map(|pair| {
                    Value::Map(vec![
                        (k::FIRST.into(), Value::Str(pair.first)),
                        (k::SECOND.into(), Value::Str(pair.second)),
                        (k::DISTANCE.into(), Value::Num(pair.distance)),
                        (k::VISION.into(), Value::Str(pair.vision.into())),
                    ])
                })
                .collect(),
        ))
    });
    m.records(&[(
        "ContrastPair",
        "{ role: string, ink: string, fill: string, ratio: float, need: float }",
        "One ink drawn on one fill: the role, the two colour names, their contrast ratio and the ratio it needs.",
    )]);
    m.function("contrast", |_: &Engine, (a, b): (String, String)| {
        let (Some(a), Some(b)) = (parse_hex(&a), parse_hex(&b)) else {
            anyhow::bail!("contrast takes two #rrggbb colours, got '{a}' and '{b}'");
        };
        Ok(Value::Num(ratio(a, b)))
    });
    m.function(
        "contrast_pairs",
        |_: &Engine, (doc, ground): (Value, String)| {
            let doc = balaur_core::node_api::to_toml(&doc)?;
            let pairs = pairs(&doc, &ground)
                .into_iter()
                .map(|pair| {
                    Value::Map(vec![
                        (k::ROLE.into(), Value::Str(pair.role)),
                        (k::INK.into(), Value::Str(pair.ink)),
                        (k::FILL.into(), Value::Str(pair.fill)),
                        (k::RATIO.into(), Value::Num(pair.ratio)),
                        (k::NEED.into(), Value::Num(pair.need)),
                    ])
                })
                .collect();
            Ok(Value::List(pairs))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{AA, AA_LARGE, pairs, ratio};
    use egui::Color32;

    #[test]
    fn black_on_white_is_twenty_one_to_one() {
        let r = ratio(Color32::BLACK, Color32::WHITE);
        assert!((r - 21.0).abs() < 1e-9, "got {r}");
    }

    #[test]
    fn a_state_table_inherits_its_role_s_fill_and_named_size() {
        let doc: toml::Value = toml::from_str(
            "[colors]\nbg_panel = \"#ffffff\"\ninkish = \"#777777\"\n\n\
             [sizes]\nfont_size = 13\n\n\
             [roles.tab]\ntext_color = \"inkish\"\nfont_size = \"font_size_title\"\n\n\
             [roles.tab.hover]\ntext_color = \"#000000\"\n",
        )
        .unwrap();
        let found = pairs(&doc, "bg_panel");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].fill, "bg_panel");
        assert!((found[0].need - AA_LARGE).abs() < f64::EPSILON);
        assert_eq!(found[1].role, "tab.hover");
        assert_eq!(found[1].fill, "bg_panel", "the hover keeps the role's fill");
        assert!(found[1].ratio > AA);
    }
}
