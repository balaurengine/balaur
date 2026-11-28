//! A theme file held to the shape each key takes: a value the reader would
//! drop, a word outside its list, a number where a colour goes, a key no
//! table reads or a state's own `transition`, said once as the file is read
//! with the table, the key, the value and what it takes. A theme still reads
//! what it can.

use super::Tokens;
use crate::vocabulary::keys as k;
use crate::vocabulary::looks as lk;
use crate::vocabulary::states as st;
use crate::vocabulary::tokens as t;
use crate::vocabulary::words as w;
use balaur_text::vocabulary::words as tw;

/// What one key of a kind's, a role's or a state's table takes.
#[derive(Clone, Copy)]
enum Shape {
    Color,
    /// A number, or the name of a size `[sizes]` states.
    Number,
    /// A number, a size's name or `full`.
    Radius,
    /// A number, a size's name or a share of the box, `"50%"`.
    Size,
    /// A number, a size's name or four numbers.
    Padding,
    Bool,
    Text,
    Pair,
    Four,
    Words(&'static [&'static str]),
    TextList,
    /// A state's table: read as a table of its own, and eased by its entry.
    State,
    /// A part's table, `body`.
    Table,
    Gradient,
    Shadows,
}

/// A share written `"50%"`, as a part of one: `None` for anything else, or
/// for nothing or less.
pub(crate) fn share_of(value: &toml::Value) -> Option<f32> {
    let percent: f32 = value
        .as_str()?
        .strip_suffix(w::track::PERCENT)?
        .trim()
        .parse()
        .ok()?;
    (percent.is_finite() && percent > 0.0).then_some(percent / 100.0)
}

/// Every key a theme's style table reads, and its shape.
const KEYS: &[(&str, Shape)] = &[
    (k::FILL, Shape::Color),
    (k::STROKE, Shape::Color),
    (k::OUTLINE, Shape::Color),
    (k::BACKDROP, Shape::Color),
    (k::TEXT_COLOR, Shape::Color),
    (k::ICON_FILL, Shape::Color),
    (k::ICON_COLOR, Shape::Color),
    (k::OUTLINE_WIDTH, Shape::Number),
    (k::OUTLINE_OFFSET, Shape::Number),
    (k::DURATION, Shape::Number),
    (k::ROW_HEIGHT, Shape::Number),
    (k::STROKE_WIDTH, Shape::Number),
    (k::ICON_SIZE, Shape::Size),
    (k::FONT_SIZE, Shape::Number),
    (k::FONT_WEIGHT, Shape::Number),
    (k::HEIGHT, Shape::Number),
    (k::WIDTH, Shape::Number),
    (k::MIN_WIDTH, Shape::Number),
    (k::MAX_WIDTH, Shape::Number),
    (k::MIN_HEIGHT, Shape::Number),
    (k::MAX_HEIGHT, Shape::Number),
    (k::PADDING_X, Shape::Number),
    (k::PADDING_Y, Shape::Number),
    (k::GAP, Shape::Number),
    (k::LETTER_SPACING, Shape::Number),
    (k::TRANSITION, Shape::Number),
    (k::CORNER_RADIUS, Shape::Radius),
    (k::PADDING, Shape::Padding),
    (k::DASHED, Shape::Bool),
    (k::SHOW_TEXT, Shape::Bool),
    (k::SHOW_ICON, Shape::Bool),
    (k::TRUNCATE, Shape::Bool),
    (k::IMAGE, Shape::Text),
    (k::ARROW, Shape::Text),
    (k::FONT, Shape::Text),
    (k::SLICE, Shape::Four),
    (k::BORDER, Shape::Four),
    (k::OFFSET, Shape::Pair),
    (k::ROTATION, Shape::Number),
    (k::ROTATION_Y, Shape::Number),
    (k::SCALE, Shape::Pair),
    (k::TEXT_ALIGN, Shape::Words(w::ALIGNS)),
    (k::FLEX_DIRECTION, Shape::Words(w::FLEX_DIRECTIONS)),
    (k::CURSOR, Shape::Words(w::cursor::ALL)),
    (k::TEXT_TRANSFORM, Shape::Words(tw::CASES)),
    (k::FIT, Shape::TextList),
    (st::HOVER, Shape::State),
    (st::ACTIVE, Shape::State),
    (st::DISABLED, Shape::State),
    (st::FOCUS, Shape::State),
    (k::CHECKED, Shape::State),
    (k::BODY, Shape::Table),
    (lk::GRADIENT, Shape::Gradient),
    (lk::SHADOWS, Shape::Shadows),
];

/// The keys a state table drops: a state paints, and never moves the layout.
/// `checked` alone keeps `font_weight`, since a write checks a widget, never
/// the pointer, and a checked widget is measured with it.
const LAYOUT: &[&str] = &[
    k::PADDING,
    k::PADDING_X,
    k::PADDING_Y,
    k::HEIGHT,
    k::WIDTH,
    k::MIN_WIDTH,
    k::MAX_WIDTH,
    k::MIN_HEIGHT,
    k::MAX_HEIGHT,
    k::FONT_SIZE,
    k::FONT,
    k::FONT_WEIGHT,
    k::LETTER_SPACING,
    k::TEXT_TRANSFORM,
    k::FIT,
    k::SHOW_TEXT,
    k::SHOW_ICON,
    k::TRUNCATE,
];

fn is_state(key: &str) -> bool {
    KEYS.iter()
        .any(|(name, shape)| *name == key && matches!(shape, Shape::State))
}

/// Whether `key` is one a style table reads, rather than a class word's.
pub(super) fn is_own(key: &str) -> bool {
    KEYS.iter().any(|(name, _)| *name == key)
}

/// The lines said about one theme document.
struct Said(Vec<String>);

impl Said {
    fn dropped(&mut self, table: &str, key: &str, value: &toml::Value, takes: &str) {
        let table = if table.is_empty() {
            String::new()
        } else {
            format!("[{table}] ")
        };
        self.0.push(format!(
            "{table}`{key}` = {value} is not read as written: it takes {takes}"
        ));
    }
}

/// What the theme says that its reader would drop or read otherwise, one
/// line a value: `table` as the palette completed it, `written` as the file
/// states its sources.
pub(super) fn dropped(table: &toml::Table, written: &toml::Table, tokens: &Tokens) -> Vec<String> {
    let mut said = Said(Vec::new());
    for (kind, body) in table {
        if super::is_reserved(kind) {
            continue;
        }
        match body.as_table() {
            Some(body) => style(body, tokens, &mut said, kind, false),
            None => said.dropped("", kind, body, "a table: a widget kind's, `[button]`"),
        }
    }
    if let Some(roles) = table.get(k::ROLES).and_then(toml::Value::as_table) {
        for (name, body) in roles {
            match body.as_table() {
                Some(body) => style(
                    body,
                    tokens,
                    &mut said,
                    &format!("{}.{name}", k::ROLES),
                    false,
                ),
                None => said.dropped(k::ROLES, name, body, "a table: a role's"),
            }
        }
    }
    sources(written, &mut said);
    said.0
}

/// The palette's sources as the file states them: sizes, hues and mixes are
/// numbers, a colour is `#rrggbb` and `contrast` a number.
fn sources(written: &toml::Table, said: &mut Said) {
    for (group, takes) in [
        (k::SIZES, "a number of design pixels"),
        (k::HUES, "a hue in degrees"),
        (k::MIXES, "a share from 0 to 1"),
    ] {
        for (name, value) in written
            .get(group)
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
        {
            if !is_number(value) {
                said.dropped(group, name, value, takes);
            }
        }
    }
    for (name, value) in written
        .get(k::COLORS)
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        let (fits, takes) = if name == t::CONTRAST {
            (
                is_number(value),
                "a number, how far apart the surfaces step",
            )
        } else {
            (
                value.as_str().and_then(super::parse_color).is_some(),
                "a colour, `#rrggbb` or `#rrggbbaa`",
            )
        };
        if !fits {
            said.dropped(k::COLORS, name, value, takes);
        }
    }
}

/// One style table, and the tables inside it: its states, its parts and a
/// class word's. A state's own `transition` was said by its entry.
fn style(body: &toml::Table, tokens: &Tokens, said: &mut Said, table: &str, state: bool) {
    for (key, value) in body {
        if state && key == k::TRANSITION {
            continue;
        }
        // `checked` on a kind, a role or a class word; one inside a state is read by nothing.
        let mut path = table.rsplit('.');
        let checked_weight = key == k::FONT_WEIGHT
            && path.next() == Some(k::CHECKED)
            && !path.next().is_some_and(is_state);
        if state && LAYOUT.contains(&key.as_str()) && !checked_weight {
            said.dropped(
                table,
                key,
                value,
                "nothing in a state, which paints: it goes on the kind or the role",
            );
            continue;
        }
        let inner = || format!("{table}.{key}");
        let Some((_, shape)) = KEYS.iter().find(|(name, _)| name == key) else {
            match value.as_table() {
                // A class word's table, `[button.narrow]`.
                Some(fields) => style(fields, tokens, said, &inner(), false),
                None => said.dropped(table, key, value, "nothing: no theme table reads this key"),
            }
            continue;
        };
        match (shape, value.as_table()) {
            (Shape::State, Some(fields)) => {
                if let Some(eased) = fields.get(k::TRANSITION) {
                    said.dropped(
                        &inner(),
                        k::TRANSITION,
                        eased,
                        &format!("a place on [{table}] itself, which eases every state"),
                    );
                }
                style(fields, tokens, said, &inner(), true);
            }
            (Shape::Table, Some(fields)) => style(fields, tokens, said, &inner(), false),
            (Shape::State | Shape::Table, None) => said.dropped(table, key, value, "a table"),
            (Shape::Gradient, _) => gradient(value, tokens, said, table),
            (Shape::Shadows, _) => shadows(value, tokens, said, table),
            (shape, _) => {
                if let Some(takes) = refused(*shape, value, tokens) {
                    said.dropped(table, key, value, &takes);
                }
            }
        }
    }
}

fn is_number(value: &toml::Value) -> bool {
    balaur_core::components::as_f64(value).is_some()
}

fn numbers(value: &toml::Value, count: usize) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.len() == count && items.iter().all(is_number))
}

fn is_color(value: &toml::Value, tokens: &Tokens) -> bool {
    super::color(value, &tokens.colors).is_some()
}

fn one_of(words: &[&str]) -> String {
    let listed: Vec<String> = words.iter().map(|word| format!("`{word}`")).collect();
    format!("one of {}", listed.join(", "))
}

/// What `shape` takes where `value` is not that, or none where it is.
fn refused(shape: Shape, value: &toml::Value, tokens: &Tokens) -> Option<String> {
    let (fits, takes) = match shape {
        Shape::Color => (
            is_color(value, tokens),
            "a colour, `#rrggbb` or `#rrggbbaa`, a token `[colors]` names, or `none`",
        ),
        Shape::Number => (
            tokens.number(value).is_some(),
            "a number, or a size `[sizes]` names",
        ),
        Shape::Radius => (
            tokens.number(value).is_some() || value.as_str() == Some(w::FULL),
            "a number, a size `[sizes]` names, or `full`",
        ),
        Shape::Size => (
            tokens.number(value).is_some() || share_of(value).is_some(),
            "a number, a size `[sizes]` names, or a share of the box as `\"50%\"`",
        ),
        Shape::Padding => (
            tokens.number(value).is_some() || numbers(value, 4),
            "a number, a size `[sizes]` names, or four numbers",
        ),
        Shape::Bool => (value.is_bool(), "`true` or `false`"),
        Shape::Text => (value.is_str(), "text"),
        Shape::Pair => (numbers(value, 2), "two numbers, `[x, y]`"),
        Shape::Four => (
            numbers(value, 4),
            "four numbers, `[left, top, right, bottom]`",
        ),
        Shape::Words(words) => {
            let fits = value.as_str().is_some_and(|word| words.contains(&word));
            return (!fits).then(|| one_of(words));
        }
        Shape::TextList => (
            value
                .as_array()
                .is_some_and(|items| items.iter().all(toml::Value::is_str)),
            "a list of words",
        ),
        Shape::State | Shape::Table | Shape::Gradient | Shape::Shadows => return None,
    };
    (!fits).then(|| takes.to_string())
}

fn gradient(value: &toml::Value, tokens: &Tokens, said: &mut Said, table: &str) {
    let Some(fields) = value.as_table() else {
        said.dropped(table, lk::GRADIENT, value, "a table, `{ stops = [..] }`");
        return;
    };
    let stops = fields
        .get(lk::STOPS)
        .and_then(toml::Value::as_array)
        .map_or(0, Vec::len);
    if stops < 2 {
        said.dropped(table, lk::GRADIENT, value, "two `stops` colours at least");
    }
    for (key, value) in fields {
        let fits = match key.as_str() {
            lk::KIND => [lk::LINEAR, lk::RADIAL].contains(&value.as_str().unwrap_or_default()),
            lk::STOPS => value.as_array().is_some_and(|stops| {
                stops.len() >= 2 && stops.iter().all(|stop| is_color(stop, tokens))
            }),
            lk::AT => value
                .as_array()
                .is_some_and(|places| places.len() == stops && places.iter().all(is_number)),
            lk::ANGLE | lk::RADIUS => is_number(value),
            lk::CENTER => numbers(value, 2),
            _ => false,
        };
        if !fits {
            said.dropped(
                table,
                &format!("{}.{key}", lk::GRADIENT),
                value,
                "what a gradient reads: `kind` (`linear` or `radial`), two `stops` colours at least, `at` a number for each stop, `angle`, `center` and `radius`",
            );
        }
    }
}

fn shadows(value: &toml::Value, tokens: &Tokens, said: &mut Said, table: &str) {
    let Some(items) = value.as_array() else {
        said.dropped(
            table,
            lk::SHADOWS,
            value,
            "a list, `[{ y = 2, blur = 8, color = \"#00000040\" }]`",
        );
        return;
    };
    for item in items {
        let fits = item.as_table().is_some_and(|shadow| {
            shadow
                .get(lk::COLOR)
                .is_some_and(|color| is_color(color, tokens))
                && shadow.iter().all(|(key, value)| match key.as_str() {
                    lk::X | lk::Y | lk::BLUR | lk::SPREAD => is_number(value),
                    lk::COLOR => true,
                    lk::INSET => value.is_bool(),
                    _ => false,
                })
        });
        if !fits {
            said.dropped(
                table,
                lk::SHADOWS,
                item,
                "shadows each with a `color` and numbers `x`, `y`, `blur` and `spread`, and `inset` true or false",
            );
        }
    }
}
