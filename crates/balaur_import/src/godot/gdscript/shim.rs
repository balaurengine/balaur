//! `gd.rn`: Godot's Variant semantics as one Rune module.
//!
//! Godot answers `size()`, `is_empty()` and `get()` on strings, arrays,
//! dictionaries and objects alike, and Rune's types do not. Rather than infer
//! a static type for every expression, the emitter routes those calls here and
//! this dispatches on the value.

/// The text written to `gd.rn` beside a converted project.
pub(crate) const SHIM: &str = r#"// Godot's Variant verbs, for scripts `balaur import` converted. Written by
// the importer: edit the importer, not this file.

/// Godot's `null`, which is Rune's unit.
pub fn is_nil(value) {
    value is Tuple
}

pub fn valid(value) {
    if value is Tuple {
        return false;
    }
    if value is balaur::Node {
        return value.is_valid();
    }
    true
}

pub fn size(value) {
    if value is String {
        return value.len();
    }
    if value is Vec {
        return value.len();
    }
    if value is Object {
        return value.keys().len();
    }
    0
}

pub fn is_empty(value) {
    size(value) == 0
}

/// `dict.get(key, default)`, and `array.get(index)`.
pub fn get(container, key, default) {
    if container is Object {
        if container.contains_key(key) {
            return container[key];
        }
        return default;
    }
    if container is Vec {
        if key is i64 && key >= 0 && key < container.len() {
            return container[key];
        }
        return default;
    }
    default
}

pub fn has(container, value) {
    if container is Object {
        return container.contains_key(value);
    }
    if container is Vec {
        for item in container {
            if item == value {
                return true;
            }
        }
        return false;
    }
    if container is String {
        return container.contains(value);
    }
    false
}

pub fn keys(container) {
    if container is Object {
        return container.keys();
    }
    []
}

pub fn values(container) {
    if container is Object {
        return container.values();
    }
    if container is Vec {
        return container;
    }
    []
}

pub fn append(container, value) {
    container.push(value);
    container
}

pub fn append_array(container, other) {
    for item in other {
        container.push(item);
    }
    container
}

pub fn erase(container, value) {
    if container is Object {
        container.remove(value);
        return container;
    }
    let out = [];
    for item in container {
        if item != value {
            out.push(item);
        }
    }
    out
}

pub fn find(container, value) {
    let index = 0;
    for item in container {
        if item == value {
            return index;
        }
        index += 1;
    }
    -1
}

pub fn clear(container) {
    if container is Object {
        for key in container.keys() {
            container.remove(key);
        }
        return container;
    }
    container.clear();
    container
}

pub fn duplicate(value) {
    if value is Vec {
        let out = [];
        for item in value {
            out.push(item);
        }
        return out;
    }
    if value is Object {
        let out = #{};
        for key in value.keys() {
            out[key] = value[key];
        }
        return out;
    }
    value
}

pub fn merge(into, other) {
    for key in other.keys() {
        into[key] = other[key];
    }
    into
}

pub fn front(container) {
    if size(container) == 0 {
        return ();
    }
    container[0]
}

pub fn back(container) {
    let count = size(container);
    if count == 0 {
        return ();
    }
    container[count - 1]
}

pub fn pop_front(container) {
    if size(container) == 0 {
        return ();
    }
    container.remove(0)
}

pub fn pop_back(container) {
    container.pop()
}

pub fn sort(container) {
    container.sort();
    container
}

pub fn reverse(container) {
    container.reverse();
    container
}

pub fn slice(container, from, upto) {
    let count = size(container);
    let start = if from < 0 { count + from } else { from };
    let end = if upto < 0 { count + upto } else { upto };
    let out = [];
    let index = start;
    while index < end && index < count {
        out.push(container[index]);
        index += 1;
    }
    out
}

/// A map built from pairs, for a literal whose keys are not literal strings.
pub fn dict(pairs) {
    let out = #{};
    for pair in pairs {
        out[pair[0]] = pair[1];
    }
    out
}

pub fn str(value) {
    if value is String {
        return value;
    }
    format!("{}", value)
}

pub fn str_all(values) {
    let out = "";
    for value in values {
        if out != "" {
            out += " ";
        }
        out += str(value);
    }
    out
}

pub fn int(value) {
    if value is i64 {
        return value;
    }
    if value is f64 {
        return value as i64;
    }
    if value is bool {
        return if value { 1 } else { 0 };
    }
    if value is String {
        let parsed = value.parse::<i64>();
        if parsed is Result {
            return parsed.unwrap_or(0);
        }
        return 0;
    }
    0
}

pub fn float(value) {
    if value is f64 {
        return value;
    }
    if value is i64 {
        return value as f64;
    }
    if value is String {
        let parsed = value.parse::<f64>();
        if parsed is Result {
            return parsed.unwrap_or(0.0);
        }
        return 0.0;
    }
    0.0
}

pub fn bool(value) {
    if value is bool {
        return value;
    }
    if value is i64 {
        return value != 0;
    }
    if value is f64 {
        return value != 0.0;
    }
    if value is String {
        return value != "";
    }
    !is_nil(value)
}

pub fn to_lower(value) {
    value.to_lowercase()
}

pub fn to_upper(value) {
    value.to_uppercase()
}

pub fn strip_edges(value) {
    value.trim()
}

pub fn begins_with(value, prefix) {
    value.starts_with(prefix)
}

pub fn ends_with(value, suffix) {
    value.ends_with(suffix)
}

pub fn contains(value, part) {
    value.contains(part)
}

pub fn split(value, sep) {
    value.split(sep).collect::<Vec>()
}

pub fn join(sep, parts) {
    let out = "";
    let first = true;
    for part in parts {
        if !first {
            out += sep;
        }
        out += str(part);
        first = false;
    }
    out
}

pub fn substr(value, from, count) {
    let total = value.len();
    let start = if from < 0 { 0 } else { from };
    let end = if count < 0 { total } else { start + count };
    let out = "";
    let index = 0;
    for c in value.chars() {
        if index >= start && index < end {
            out += c;
        }
        index += 1;
    }
    out
}

/// Godot's `"%s of %d" % [a, b]`. The verbs it uses in this game are `%s`,
/// `%d`, `%f`, `%.Nf` and `%%`.
pub fn format(template, args) {
    let out = "";
    let index = 0;
    let chars = template.chars().collect::<Vec>();
    let at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c != '%' {
            out += c;
            at += 1;
            continue;
        }
        at += 1;
        if at >= chars.len() {
            break;
        }
        if chars[at] == '%' {
            out += "%";
            at += 1;
            continue;
        }
        let spec = "";
        while at < chars.len() && chars[at] != 's' && chars[at] != 'd' && chars[at] != 'f'
            && chars[at] != 'x' && chars[at] != 'v' {
            spec += chars[at];
            at += 1;
        }
        let verb = if at < chars.len() { chars[at] } else { 's' };
        at += 1;
        let value = if index < args.len() { args[index] } else { () };
        index += 1;
        if verb == 'd' {
            out += str(int(value));
        } else if verb == 'f' {
            out += decimals(float(value), places(spec));
        } else {
            out += str(value);
        }
    }
    out
}

/// `"{a}".format({"a": 1})`, Godot's other formatter.
pub fn format_map(template, values) {
    let out = template;
    for key in keys(values) {
        out = out.replace(`{${key}}`, str(values[key]));
    }
    out
}

/// The digits a `%.3f` asks for; 6 is Godot's default.
fn places(spec) {
    let dot = false;
    let digits = "";
    for c in spec.chars() {
        if c == '.' {
            dot = true;
            continue;
        }
        if dot {
            digits += c;
        }
    }
    if digits == "" {
        return 6;
    }
    int(digits)
}

fn decimals(value, count) {
    let scale = 1.0;
    let step = 0;
    while step < count {
        scale *= 10.0;
        step += 1;
    }
    let rounded = math::round(value * scale) / scale;
    let text = format!("{}", rounded);
    if count == 0 {
        return str(int(rounded));
    }
    let dot = text.find('.');
    if is_nil(dot) {
        text += ".";
        let pad = 0;
        while pad < count {
            text += "0";
            pad += 1;
        }
        return text;
    }
    text
}

pub fn lerp(from, to, amount) {
    from + (to - from) * amount
}

pub fn sign(value) {
    if value > 0.0 {
        return 1.0;
    }
    if value < 0.0 {
        return -1.0;
    }
    0.0
}

pub fn snapped(value, step) {
    if step == 0.0 {
        return value;
    }
    math::round(value / step) * step
}

pub fn fmod(value, by) {
    value - by * math::floor(value / by)
}

pub fn move_toward(from, to, delta) {
    if math::abs(to - from) <= delta {
        return to;
    }
    from + sign(to - from) * delta
}

pub fn is_zero_approx(value) {
    math::abs(value) < 0.00001
}

pub fn is_equal_approx(a, b) {
    math::abs(a - b) < 0.00001
}

pub fn nan() {
    0.0 / 0.0
}

pub fn set_position(node, at) {
    node.set_position(float(at.x), float(at.y), 0.0);
}

pub fn set_global_position(node, at) {
    let here = node.global_position();
    let parent = node.position();
    node.set_position(
        float(parent.x) + float(at.x) - float(here.x),
        float(parent.y) + float(at.y) - float(here.y),
        0.0,
    );
}

pub fn set_scale(node, by) {
    node.set_scale(float(by.x), float(by.y), 1.0);
}

pub fn set_tint(node, tint) {
    node.set_tint(float(tint.r), float(tint.g), float(tint.b), float(tint.a));
}

pub fn rect(x, y, w, h) {
    #{ "x": x, "y": y, "w": w, "h": h }
}

/// `Array()`, `Dictionary()` and the packed arrays, which Godot spelled as
/// constructors.
pub fn empty_of() {
    []
}

pub fn vec2(x, y) {
    #{ "x": x, "y": y }
}

pub fn vec3(x, y, z) {
    #{ "x": x, "y": y, "z": z }
}

pub fn color(r, g, b, a) {
    #{ "r": r, "g": g, "b": b, "a": a }
}

pub fn range(from, upto, step) {
    let out = [];
    let at = from;
    while (step > 0 && at < upto) || (step < 0 && at > upto) {
        out.push(at);
        at += step;
    }
    out
}

/// A node's script answers `is_a` by the class it was converted from, which
/// the importer writes into every converted script as `CLASS_NAME`.
pub fn is_a(value, name) {
    if is_nil(value) {
        return false;
    }
    if value is balaur::Node {
        return value.has_tag(name);
    }
    false
}

pub fn inverse_lerp(from, to, value) {
    if to == from {
        return 0.0;
    }
    (value - from) / (to - from)
}

pub fn linear_to_db(value) {
    if value <= 0.0 {
        return -80.0;
    }
    math::log(value) / math::log(10.0) * 20.0
}

pub fn db_to_linear(value) {
    math::pow(10.0, value / 20.0)
}

pub fn type_of(value) {
    if is_nil(value) {
        return "nil";
    }
    if value is bool {
        return "bool";
    }
    if value is i64 {
        return "int";
    }
    if value is f64 {
        return "float";
    }
    if value is String {
        return "String";
    }
    if value is Vec {
        return "Array";
    }
    if value is Object {
        return "Dictionary";
    }
    "Object"
}
"#;
