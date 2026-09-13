//! `[[nodes.bindings.rows]]`: an event, a condition, an action and a target.
//!
//! Every action here is a call a script could make, and "convert to script"
//! in the editor writes exactly that call. This is not a second runtime: it is
//! a table the same dispatcher reads instead of a script file, so a scene can
//! be interactive before anybody opens an editor for code.

use anyhow::{Result, anyhow, bail};
use balaur_script::Value;

use crate::App;
use crate::components::ComponentDef;
use crate::hecs::Entity;
use crate::variables::{Variables, as_num};
use crate::{Engine, hooks};

/// What a binding does when its event fires and its condition holds.
///
/// One arm per action, each a call a script could make. A word nothing here
/// knows is refused at load, so a typo is an error at the scene rather than
/// silence at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `node.go(value)` on the target.
    State,
    /// `scene.set_variable(target, value)`.
    SetVariable,
    /// Add `value` to a number variable; `target` names it.
    AddVariable,
    /// `animation.play(target, value)`.
    Play,
    /// `sound.play` on the target.
    Sound,
    /// `scene.instantiate(value)` under the target.
    Spawn,
    /// `node.free()` on the target.
    Free,
    /// `scene.switch(value)`.
    Switch,
    /// `engine.open_url(value)`.
    OpenUrl,
    /// `events.emit(value)`.
    Emit,
    /// Call the method `value` on the target's script.
    Call,
    /// Show or hide the target; `value` is read as a bool.
    Visible,
}

/// The words a scene spells each action with, in the order the Events view
/// offers them.
pub const ACTIONS: &[(&str, Action)] = &[
    ("state", Action::State),
    ("set_variable", Action::SetVariable),
    ("add_variable", Action::AddVariable),
    ("play", Action::Play),
    ("sound", Action::Sound),
    ("spawn", Action::Spawn),
    ("free", Action::Free),
    ("switch", Action::Switch),
    ("open_url", Action::OpenUrl),
    ("emit", Action::Emit),
    ("call", Action::Call),
    ("visible", Action::Visible),
];

impl Action {
    fn of(word: &str) -> Option<Self> {
        ACTIONS
            .iter()
            .find(|(known, _)| *known == word)
            .map(|(_, action)| *action)
    }

    /// The word this action is written as.
    #[must_use]
    pub fn word(self) -> &'static str {
        ACTIONS
            .iter()
            .find(|(_, known)| *known == self)
            .map_or("call", |(word, _)| *word)
    }
}

/// One row.
#[derive(Clone, Debug)]
pub struct Binding {
    /// The bindable event name, with no `on_` prefix.
    pub event: String,
    /// A comparison over the scene's variables, or empty for "always".
    pub when: Option<Condition>,
    /// The condition as written, so the editor shows what the author typed.
    pub when_source: String,
    pub action: Action,
    /// A node path relative to the bound node, or a variable name for the
    /// variable actions. Empty means the bound node itself.
    pub target: String,
    pub value: Value,
}

/// The `bindings` component: every row on one node.
pub struct Bindings {
    pub rows: Vec<Binding>,
}

/// A comparison of a variable with a constant, or two of them joined.
///
/// Deliberately small: a condition is data in a scene file, so it is read by
/// the editor, shown in a row and diffed. Anything a comparison cannot say is
/// a script, and "convert to script" is one click away.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Compare {
        variable: String,
        op: Compare,
        value: Value,
    },
    All(Vec<Condition>),
    Any(Vec<Condition>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// The operators, longest first: `>=` has to be tried before `>`.
const OPERATORS: &[(&str, Compare)] = &[
    (">=", Compare::Ge),
    ("<=", Compare::Le),
    ("==", Compare::Eq),
    ("!=", Compare::Ne),
    (">", Compare::Gt),
    ("<", Compare::Lt),
];

/// Read a condition. `""` is `None`, which is always true.
///
/// # Errors
/// If a clause has no operator, or names nothing on the left.
pub fn parse_condition(text: &str) -> Result<Option<Condition>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    parse_any(text).map(Some)
}

fn parse_any(text: &str) -> Result<Condition> {
    let parts: Vec<&str> = text.split("||").collect();
    if parts.len() > 1 {
        return Ok(Condition::Any(
            parts
                .iter()
                .map(|part| parse_all(part))
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    parse_all(text)
}

fn parse_all(text: &str) -> Result<Condition> {
    let parts: Vec<&str> = text.split("&&").collect();
    if parts.len() > 1 {
        return Ok(Condition::All(
            parts
                .iter()
                .map(|part| parse_compare(part))
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    parse_compare(text)
}

fn parse_compare(text: &str) -> Result<Condition> {
    let text = text.trim();
    for (token, op) in OPERATORS {
        let Some(at) = text.find(token) else {
            continue;
        };
        let variable = text[..at].trim().to_string();
        let rest = text[at + token.len()..].trim();
        if variable.is_empty() {
            bail!("`{text}`: a condition compares a variable with a value");
        }
        return Ok(Condition::Compare {
            variable,
            op: *op,
            value: literal(rest),
        });
    }
    // A bare name is "this variable is true", which is what a bool wants.
    Ok(Condition::Compare {
        variable: text.to_string(),
        op: Compare::Eq,
        value: Value::Bool(true),
    })
}

/// A written value: a number, `true`/`false`, or text with the quotes off.
fn literal(text: &str) -> Value {
    let text = text.trim();
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(number) = text.parse::<f64>() {
        return Value::Num(number);
    }
    Value::Str(text.trim_matches(['"', '\'']).to_string())
}

impl Condition {
    /// Whether this holds against the scene's variables. A clause naming a
    /// variable nothing declared is false rather than an error: the scene was
    /// already told at load.
    #[must_use]
    pub fn holds(&self, variables: &Variables) -> bool {
        match self {
            Self::All(all) => all.iter().all(|c| c.holds(variables)),
            Self::Any(any) => any.iter().any(|c| c.holds(variables)),
            Self::Compare {
                variable,
                op,
                value,
            } => {
                let Some(held) = variables.get(variable) else {
                    return false;
                };
                compare(held, *op, value)
            }
        }
    }

    /// Every variable this reads, so the editor can list what a row depends on.
    #[must_use]
    pub fn variables(&self) -> Vec<String> {
        match self {
            Self::All(list) | Self::Any(list) => {
                list.iter().flat_map(Condition::variables).collect()
            }
            Self::Compare { variable, .. } => vec![variable.clone()],
        }
    }
}

fn compare(held: &Value, op: Compare, against: &Value) -> bool {
    // Text compares as text and everything else as a number, so `name ==
    // "ana"` and `score >= 3` both say what they look like.
    if let (Value::Str(a), Value::Str(b)) = (held, against) {
        return match op {
            Compare::Eq => a == b,
            Compare::Ne => a != b,
            Compare::Lt => a < b,
            Compare::Le => a <= b,
            Compare::Gt => a > b,
            Compare::Ge => a >= b,
        };
    }
    let a = as_num(held);
    let b = as_num(against);
    match op {
        Compare::Eq => (a - b).abs() < f64::EPSILON,
        Compare::Ne => (a - b).abs() >= f64::EPSILON,
        Compare::Lt => a < b,
        Compare::Le => a <= b,
        Compare::Gt => a > b,
        Compare::Ge => a >= b,
    }
}

fn value_of(row: &toml::Value) -> Value {
    match row {
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::String(s) => Value::Str(s.clone()),
        other => crate::components::as_f64(other).map_or(Value::Nil, Value::Num),
    }
}

/// Read one `[[nodes.bindings.rows]]` row.
///
/// # Errors
/// If the row names no event, or an action nothing knows.
pub fn parse_binding(row: &toml::Value) -> Result<Binding> {
    let text = |key: &str| {
        row.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let event = text("event");
    if event.is_empty() {
        bail!("a binding names the event it answers: `event = \"pointer_click\"`");
    }
    if !hooks::is_bindable(&event) {
        bail!(
            "no event `{event}`; the events are {}, and `{}<name>` for a name the node emits",
            hooks::BINDABLE.join(", "),
            hooks::EMITTED
        );
    }
    let word = text("action");
    let action = Action::of(&word).ok_or_else(|| {
        anyhow!(
            "no action `{word}`; the actions are {}",
            ACTIONS
                .iter()
                .map(|(w, _)| *w)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let when_source = text("when");
    Ok(Binding {
        event,
        when: parse_condition(&when_source)?,
        when_source,
        action,
        target: text("target"),
        value: row.get("value").map_or(Value::Nil, value_of),
    })
}

/// The `bindings` component, written as an array of tables on a node.
pub(crate) fn register_bindings_component(app: &mut App) {
    app.register_component(
        "bindings",
        ComponentDef {
            doc: "Reactions the node runs from a table: each row is an `event`, a `when` over the scene's `[variables]`, an `action`, a `target` node and a `value`.",
            // Written `[[nodes.bindings.rows]]` in a scene: a table with one
            // property, like every other component.
            schema: ComponentDef::parse_schema(
                "bindings",
                r#"rows = { type = "strings", default = [], description = "The binding rows, each `{ event, when, action, target, value }`" }"#,
            ),
            tags: &["interaction"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let rows = match params.get("rows") {
                    Some(toml::Value::Array(rows)) => rows
                        .iter()
                        .map(parse_binding)
                        .collect::<Result<Vec<_>>>()?,
                    None => Vec::new(),
                    Some(_) => bail!("`bindings.rows` is a list of rows"),
                };
                let mut world = eng.world_mut();
                if let Ok(mut held) = world.get::<&mut Bindings>(entity) {
                    held.rows = rows;
                    return Ok(());
                }
                world
                    .insert_one(entity, Bindings { rows })
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Bindings>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let held = world.get::<&Bindings>(entity).ok()?;
                // Under `rows`, the one property the schema declares.
                let mut map = toml::map::Map::new();
                map.insert(
                    "rows".into(),
                    toml::Value::Array(held.rows.iter().map(row_to_toml).collect()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}

fn row_to_toml(row: &Binding) -> toml::Value {
    let mut map = toml::map::Map::new();
    map.insert("event".into(), toml::Value::String(row.event.clone()));
    if !row.when_source.is_empty() {
        map.insert("when".into(), toml::Value::String(row.when_source.clone()));
    }
    map.insert(
        "action".into(),
        toml::Value::String(row.action.word().to_string()),
    );
    if !row.target.is_empty() {
        map.insert("target".into(), toml::Value::String(row.target.clone()));
    }
    let value = match &row.value {
        Value::Bool(b) => Some(toml::Value::Boolean(*b)),
        Value::Num(n) => Some(toml::Value::Float(*n)),
        Value::Str(s) => Some(toml::Value::String(s.clone())),
        _ => None,
    };
    if let Some(value) = value {
        map.insert("value".into(), value);
    }
    toml::Value::Table(map)
}

/// Run every binding on `entity` that answers `event`.
///
/// Bindings run before the node's own script hook, so a script that also
/// declares the hook sees the world the bindings left.
pub fn fire(eng: &Engine, entity: Entity, event: &str, args: &[Value]) {
    let rows = {
        let world = eng.world();
        let Ok(held) = world.get::<&Bindings>(entity) else {
            return;
        };
        held.rows
            .iter()
            .filter(|row| row.event == event)
            .cloned()
            .collect::<Vec<_>>()
    };
    for row in rows {
        if let Some(when) = &row.when {
            let variables = eng.resource::<Variables>();
            let holds = when.holds(&variables.borrow());
            if !holds {
                continue;
            }
        }
        if let Err(why) = run(eng, entity, &row, args) {
            tracing::warn!("binding `{}` on this node: {why:#}", row.event);
        }
    }
}

/// The node a row acts on: its `target` as a path from the bound node, or the
/// bound node when the row names none.
fn target_of(eng: &Engine, entity: Entity, row: &Binding) -> Result<Entity> {
    if row.target.is_empty() {
        return Ok(entity);
    }
    let world = eng.world();
    crate::scene::find_node(&world, entity, &row.target)
        .ok_or_else(|| anyhow!("`{}` names no node from here", row.target))
}

fn text_of(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Num(n) => format!("{n}"),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn run(eng: &Engine, entity: Entity, row: &Binding, args: &[Value]) -> Result<()> {
    match row.action {
        Action::State => crate::states::go(eng, target_of(eng, entity, row)?, &text_of(&row.value)),
        Action::SetVariable => {
            let variables = eng.resource::<Variables>();
            let mut variables = variables.borrow_mut();
            variables.set(&row.target, &row.value)
        }
        Action::AddVariable => {
            let variables = eng.resource::<Variables>();
            let mut variables = variables.borrow_mut();
            let held = variables.get(&row.target).map_or(0.0, as_num);
            let next = Value::Num(held + as_num(&row.value));
            variables.set(&row.target, &next)
        }
        Action::Free => {
            crate::scene::free_node(eng, target_of(eng, entity, row)?);
            Ok(())
        }
        Action::Visible => {
            let target = target_of(eng, entity, row)?;
            let on = match &row.value {
                Value::Bool(b) => *b,
                Value::Num(n) => *n != 0.0,
                _ => true,
            };
            let world = eng.world();
            if let Ok(mut appearance) = world.get::<&mut crate::Appearance>(target) {
                appearance.visible = on;
            }
            Ok(())
        }
        Action::Call => {
            let target = target_of(eng, entity, row)?;
            let Some(host) = eng.script_host() else {
                return Ok(());
            };
            host.call_on(crate::node_id_of(target), &text_of(&row.value), args);
            Ok(())
        }
        // The rest reach verbs core does not own. Each plugin that owns one
        // registers its runner at load, so `bindings` never links against
        // animation, audio or the web.
        other => {
            let target = target_of(eng, entity, row)?;
            let runner = {
                let runners = eng.resource::<Runners>();
                let runners = runners.borrow();
                runners.get(other)
            };
            match runner {
                Some(run) => run(eng, target, &row.value),
                None => bail!("nothing in this build runs the `{}` action", other.word()),
            }
        }
    }
}

/// What one action does, for the actions core cannot run itself.
pub type Runner = std::rc::Rc<dyn Fn(&Engine, Entity, &Value) -> Result<()>>;

/// The runners registered for those actions, by action.
///
/// A registry rather than an event: a binding is a call, and a call that
/// silently did nothing because a plugin was off is worse than one that says
/// so. Missing a runner is an error naming the action.
#[derive(Default)]
pub struct Runners {
    registered: Vec<(Action, Runner)>,
}

impl Runners {
    /// Say what runs `action`, replacing whatever ran it before.
    pub fn set(&mut self, action: Action, run: Runner) {
        self.registered.retain(|(known, _)| *known != action);
        self.registered.push((action, run));
    }

    /// Whether something registered a runner for `action`.
    #[must_use]
    pub fn has(&self, action: Action) -> bool {
        self.registered.iter().any(|(known, _)| *known == action)
    }

    fn get(&self, action: Action) -> Option<Runner> {
        self.registered
            .iter()
            .find(|(known, _)| *known == action)
            .map(|(_, run)| run.clone())
    }
}

/// Register a runner for one action.
pub fn set_runner(eng: &Engine, action: Action, run: Runner) {
    let runners = eng.resource::<Runners>();
    runners.borrow_mut().set(action, run);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variables::{VarType, Variables};

    fn variables(rows: &[(&str, VarType, Value)]) -> Variables {
        let mut variables = Variables::default();
        for (name, kind, value) in rows {
            variables.declare(name, *kind, value, false);
        }
        variables
    }

    #[test]
    fn a_comparison_reads_the_variable_it_names() {
        let held = variables(&[("score", VarType::Int, Value::Num(3.0))]);
        let condition = parse_condition("score >= 3").unwrap().unwrap();
        assert!(condition.holds(&held));
        assert!(!parse_condition("score > 3").unwrap().unwrap().holds(&held));
        assert!(parse_condition("score != 4").unwrap().unwrap().holds(&held));
    }

    #[test]
    fn a_bare_name_asks_whether_the_flag_is_set() {
        let held = variables(&[("open", VarType::Bool, Value::Bool(true))]);
        assert!(parse_condition("open").unwrap().unwrap().holds(&held));
    }

    #[test]
    fn clauses_join_with_and_and_or() {
        let held = variables(&[
            ("score", VarType::Int, Value::Num(5.0)),
            ("lives", VarType::Int, Value::Num(0.0)),
        ]);
        assert!(
            parse_condition("score >= 3 && lives == 0")
                .unwrap()
                .unwrap()
                .holds(&held)
        );
        assert!(
            !parse_condition("score >= 9 && lives == 0")
                .unwrap()
                .unwrap()
                .holds(&held)
        );
        assert!(
            parse_condition("score >= 9 || lives == 0")
                .unwrap()
                .unwrap()
                .holds(&held)
        );
    }

    /// A condition over a name nothing declared is false, not an error: a
    /// level without that variable simply never satisfies the row.
    #[test]
    fn an_undeclared_variable_never_satisfies_a_row() {
        let held = variables(&[]);
        assert!(!parse_condition("score >= 1").unwrap().unwrap().holds(&held));
    }

    #[test]
    fn text_compares_as_text() {
        let held = variables(&[("who", VarType::Text, Value::Str("ana".into()))]);
        assert!(
            parse_condition("who == \"ana\"")
                .unwrap()
                .unwrap()
                .holds(&held)
        );
        assert!(
            !parse_condition("who == \"bob\"")
                .unwrap()
                .unwrap()
                .holds(&held)
        );
    }

    #[test]
    fn a_row_names_an_event_and_an_action_this_build_knows() {
        let row: toml::Value = toml::from_str(
            r#"
event = "pointer_click"
when = "score >= 3"
action = "state"
target = "../Door"
value = "open"
"#,
        )
        .unwrap();
        let binding = parse_binding(&row).unwrap();
        assert_eq!(binding.event, "pointer_click");
        assert_eq!(binding.action, Action::State);
        assert_eq!(binding.target, "../Door");
        assert_eq!(binding.value, Value::Str("open".into()));
        assert!(binding.when.is_some());

        let bad: toml::Value =
            toml::from_str("event = \"pointer_click\"\naction = \"teleport\"").unwrap();
        let err = parse_binding(&bad).unwrap_err().to_string();
        assert!(err.contains("no action `teleport`"), "{err}");

        let unknown: toml::Value =
            toml::from_str("event = \"telepathy\"\naction = \"state\"").unwrap();
        let err = parse_binding(&unknown).unwrap_err().to_string();
        assert!(err.contains("no event `telepathy`"), "{err}");
    }

    /// What the component writes back is what a scene file would say, so a
    /// save through the editor is the file the author wrote.
    #[test]
    fn a_row_round_trips_through_the_component() {
        let row: toml::Value = toml::from_str(
            r#"
event = "pointer_enter"
action = "set_variable"
target = "score"
value = 3.0
"#,
        )
        .unwrap();
        let binding = parse_binding(&row).unwrap();
        let written = row_to_toml(&binding);
        let again = parse_binding(&written).unwrap();
        assert_eq!(again.event, binding.event);
        assert_eq!(again.action, binding.action);
        assert_eq!(again.target, binding.target);
        assert_eq!(again.value, binding.value);
    }
}
