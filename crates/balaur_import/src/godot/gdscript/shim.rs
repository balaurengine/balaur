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
    return value is Tuple;
}

pub fn valid(value) {
    if value is Tuple {
        return false;
    }
    if value is balaur::Node {
        return value.is_valid();
    }
    return true;
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
    return 0;
}

pub fn is_empty(value) {
    return size(value) == 0;
}

/// `dict.get(key, fallback)`, and `array.get(index)`. `default` is a Rune
/// keyword, so the parameter is not called that.
pub fn get(container, key, fallback) {
    if container is Object {
        if container.contains_key(key) {
            return container[key];
        }
        return fallback;
    }
    if container is Vec {
        if key is i64 && key >= 0 && key < container.len() {
            return container[key];
        }
        return fallback;
    }
    return fallback;
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
    return false;
}

pub fn keys(container) {
    if container is Object {
        return container.keys();
    }
    return [];
}

pub fn values(container) {
    if container is Object {
        return container.values();
    }
    if container is Vec {
        return container;
    }
    return [];
}

pub fn append(container, value) {
    container.push(value);
    return container;
}

pub fn append_array(container, other) {
    for item in other {
        container.push(item);
    }
    return container;
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
    return out;
}

pub fn find(container, value) {
    // On a string Godot's `find` is the index of a substring, not a search
    // through elements. Rune's String has no `find`, so the text before the
    // first split is the index.
    if container is String {
        if !container.contains(value) {
            return -1;
        }
        let head = container.split(value).collect::<Vec>();
        return head[0].len();
    }
    let index = 0;
    for item in container {
        if item == value {
            return index;
        }
        index += 1;
    }
    return -1;
}

pub fn clear(container) {
    if container is Object {
        for key in container.keys() {
            container.remove(key);
        }
        return container;
    }
    container.clear();
    return container;
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
    return value;
}

pub fn merge(into, other) {
    for key in other.keys() {
        into[key] = other[key];
    }
    return into;
}

pub fn front(container) {
    if size(container) == 0 {
        return ();
    }
    return container[0];
}

pub fn back(container) {
    let count = size(container);
    if count == 0 {
        return ();
    }
    return container[count - 1];
}

pub fn pop_front(container) {
    if size(container) == 0 {
        return ();
    }
    return container.remove(0);
}

pub fn pop_back(container) {
    return container.pop();
}

pub fn sort(container) {
    container.sort();
    return container;
}

pub fn reverse(container) {
    container.reverse();
    return container;
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
    return out;
}

/// A map built from pairs, for a literal whose keys are not literal strings.
pub fn dict(pairs) {
    let out = #{};
    for pair in pairs {
        out[pair[0]] = pair[1];
    }
    return out;
}

pub fn str(value) {
    if value is String {
        return value;
    }
    return format!("{}", value);
}

pub fn str_all(values) {
    let out = "";
    for value in values {
        if out != "" {
            out += " ";
        }
        out += str(value);
    }
    return out;
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
    return 0;
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
    return 0.0;
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
    return !is_nil(value);
}

pub fn to_lower(value) {
    return value.to_lowercase();
}

pub fn to_upper(value) {
    return value.to_uppercase();
}

pub fn strip_edges(value) {
    return value.trim();
}

pub fn begins_with(value, prefix) {
    return value.starts_with(prefix);
}

pub fn ends_with(value, suffix) {
    return value.ends_with(suffix);
}

pub fn contains(value, part) {
    return value.contains(part);
}

pub fn split(value, sep) {
    return value.split(sep).collect::<Vec>();
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
    return out;
}

pub fn substr(value, from, count) {
    let total = value.len();
    let start = if from < 0 { 0 } else { from };
    let end = if count < 0 { total } else { start + count };
    let out = "";
    let index = 0;
    for c in value.chars() {
        if index >= start && index < end {
            out += str(c);
        }
        index += 1;
    }
    return out;
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
            out += str(c);
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
            spec += str(chars[at]);
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
    return out;
}

/// `"{a}".format({"a": 1})`, Godot's other formatter.
pub fn format_map(template, values) {
    let out = template;
    for key in keys(values) {
        out = out.replace(`{${key}}`, str(values[key]));
    }
    return out;
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
            digits += str(c);
        }
    }
    if digits == "" {
        return 6;
    }
    return int(digits);
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
    return text;
}

pub fn lerp(from, to, amount) {
    return from + (to - from) * amount;
}

pub fn sign(value) {
    if value > 0.0 {
        return 1.0;
    }
    if value < 0.0 {
        return -1.0;
    }
    return 0.0;
}

pub fn snapped(value, step) {
    if step == 0.0 {
        return value;
    }
    return math::round(value / step) * step;
}

pub fn fmod(value, by) {
    return value - by * math::floor(value / by);
}

pub fn move_toward(from, to, delta) {
    if math::abs(to - from) <= delta {
        return to;
    }
    return from + sign(to - from) * delta;
}

pub fn is_zero_approx(value) {
    return math::abs(value) < 0.00001;
}

pub fn is_equal_approx(a, b) {
    return math::abs(a - b) < 0.00001;
}

pub fn nan() {
    return 0.0 / 0.0;
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
    return #{ "x": x, "y": y, "w": w, "h": h };
}

/// `Array()`, `Dictionary()` and the packed arrays, which Godot spelled as
/// constructors.
pub fn empty_of() {
    return [];
}

pub fn vec2(x, y) {
    return #{ "x": x, "y": y };
}

pub fn vec3(x, y, z) {
    return #{ "x": x, "y": y, "z": z };
}

pub fn color(r, g, b, a) {
    return #{ "r": r, "g": g, "b": b, "a": a };
}

pub fn range(from, upto, step) {
    let out = [];
    let at = from;
    while (step > 0 && at < upto) || (step < 0 && at > upto) {
        out.push(at);
        at += step;
    }
    return out;
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
    return false;
}

pub fn inverse_lerp(from, to, value) {
    if to == from {
        return 0.0;
    }
    return (value - from) / (to - from);
}

pub fn linear_to_db(value) {
    if value <= 0.0 {
        return -80.0;
    }
    return math::log(value) / math::log(10.0) * 20.0;
}

pub fn db_to_linear(value) {
    return math::pow(10.0, value / 20.0);
}

/// Godot's viewport rectangle. The engine reports the screen as a pair, and
/// the visible rect always starts at the origin.
pub fn viewport_rect() {
    let (w, h) = ui::screen_size();
    return #{ "position": vec2(0.0, 0.0), "size": vec2(w, h), "end": vec2(w, h) };
}

pub fn screen_size() {
    let (w, h) = ui::screen_size();
    return vec2(w, h);
}

/// `OS.has_feature(name)`. The engine reports where it runs as a table, not
/// as Godot's flat list of feature tags.
pub fn has_feature(name) {
    let here = engine::platform();
    if name == "editor" {
        return here.editor;
    }
    if name == "web" || name == "html5" || name == "javascript" {
        return here.web;
    }
    if name == "mobile" {
        return here.mobile;
    }
    if name == "touchscreen" {
        return here.touchscreen;
    }
    return here.os == name;
}

pub fn os_name() {
    return engine::platform().os;
}

/// Godot's `ConfigFile`: sections of keys, read from and written to a `save`
/// slot named after the file it was given. `user://settings.json` is the slot
/// `settings`, so a run given `custom_config=user://automation.cfg` keeps its
/// settings apart from the player's, as Godot's own path did.
pub fn config() {
    #{ "gd_config": true, "data": #{}, "slot": "settings" }
}

fn is_config(value) {
    value is Object && value.contains_key("gd_config")
}

pub fn slot_of(path) {
    let parts = path.split("/").collect::<Vec>();
    let name = parts[parts.len() - 1];
    let head = name.split(".").collect::<Vec>();
    return head[0];
}

pub fn config_load(owner, path) {
    if !is_config(owner) {
        return owner.load(path);
    }
    owner.slot = slot_of(path);
    let stored = save::read(owner.slot);
    owner.data = if stored is Object { stored } else { #{} };
    return 0;
}

pub fn config_save(owner, path) {
    if !is_config(owner) {
        return owner.save(path);
    }
    if path is String && path != "" {
        owner.slot = slot_of(path);
    }
    save::write(owner.slot, owner.data);
    return 0;
}

pub fn config_get(owner, section, key, fallback) {
    if !is_config(owner) {
        return owner.get_value(section, key, fallback);
    }
    return get(get(owner.data, section, #{}), key, fallback);
}

pub fn config_set(owner, section, key, value) {
    if !is_config(owner) {
        return owner.set_value(section, key, value);
    }
    let table = get(owner.data, section, #{});
    table[key] = value;
    owner.data[section] = table;
    return ();
}

pub fn config_has_section(owner, section) {
    if !is_config(owner) {
        return owner.has_section(section);
    }
    return has(owner.data, section);
}

pub fn config_has_key(owner, section, key) {
    if !is_config(owner) {
        return owner.has_section_key(section, key);
    }
    return has(get(owner.data, section, #{}), key);
}

pub fn config_erase_section(owner, section) {
    if !is_config(owner) {
        return owner.erase_section(section);
    }
    owner.data.remove(section);
    return ();
}

pub fn config_sections(owner) {
    if !is_config(owner) {
        return owner.get_sections();
    }
    return keys(owner.data);
}

pub fn config_keys(owner, section) {
    if !is_config(owner) {
        return owner.get_section_keys(section);
    }
    return keys(get(owner.data, section, #{}));
}

/// A GDScript `static var`. A Rune module holds no state, so the value lives
/// on the scene root's `meta` under a key naming the script it came from.
/// Node meta holds scalars, not nested tables, so a list or a table is kept
/// as JSON text behind this mark.
const JSON_MARK = "gdjson:";

pub fn static_get(key, fallback) {
    let root = scene::root();
    if is_nil(root) {
        return fallback;
    }
    // A meta key that was never written reads back as nil, so indexing is the
    // whole test; `meta` is a component handle and answers no `contains_key`.
    let stored = root.meta[key];
    if is_nil(stored) {
        return fallback;
    }
    if stored is String && stored.starts_with(JSON_MARK) {
        return json::parse(substr(stored, JSON_MARK.len(), -1));
    }
    return stored;
}

pub fn static_set(key, value) {
    let root = scene::root();
    if is_nil(root) {
        return value;
    }
    if value is Object || value is Vec {
        root.meta[key] = JSON_MARK + json::encode(value);
    } else {
        root.meta[key] = value;
    }
    return value;
}

/// Reading a property off another script. Godot read one script's member
/// straight off the node; here a node's script is reached by calling it, and
/// every converted script carries an accessor per member.
pub fn field(owner, name) {
    if is_nil(owner) {
        return ();
    }
    if owner is balaur::Node {
        if owner.has_method(name) {
            return owner.call(name);
        }
        return ();
    }
    if owner is Object {
        return get(owner, name, ());
    }
    ()
}

/// Writing a property on another script, through the setter every converted
/// script carries.
pub fn set_field(owner, name, value) {
    if is_nil(owner) {
        return ();
    }
    if owner is balaur::Node {
        let setter = `set_${name}`;
        if owner.has_method(setter) {
            return owner.call(setter, value);
        }
        return ();
    }
    if owner is Object {
        owner[name] = value;
    }
    ()
}

/// A call `balaur import` could not translate. It says so once per name and
/// carries on, so a scenario reaches the next gap rather than stopping here.
pub fn todo(what) {
    log::error(`balaur import did not translate ${what}`);
    return ();
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
    return "Object";
}
"#;
