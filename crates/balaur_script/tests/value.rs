//! The neutral value type and its conversions.
//!
//! Every binding argument in the engine passes through these, in both
//! languages, so a mistake here is a mistake everywhere at once.

use balaur_script::{
    expect_arity, Bindings, BindingsExt, CallbackId, FromArg, FromArgs, IntoValue, NoBindings,
    NodeId, Value,
};

#[test]
fn numbers_convert_in_both_directions() {
    assert_eq!(i64::from_arg(Some(&Value::Int(7))).unwrap(), 7);
    assert!((f32::from_arg(Some(&Value::Num(1.5))).unwrap() - 1.5).abs() < f32::EPSILON);
    assert!((f64::from_arg(Some(&Value::Int(3))).unwrap() - 3.0).abs() < f64::EPSILON);
    assert_eq!(7i64.into_value(), Value::Int(7));
}

/// A script writes `1` where a float is wanted more often than not, so an
/// integer has to be accepted as one.
#[test]
fn an_integer_is_accepted_where_a_float_is_wanted() {
    assert!((f32::from_arg(Some(&Value::Int(2))).unwrap() - 2.0).abs() < f32::EPSILON);
    assert!(usize::from_arg(Some(&Value::Int(4))).is_ok());
}

#[test]
fn strings_and_bools_convert() {
    assert_eq!(
        String::from_arg(Some(&Value::Str("hi".into()))).unwrap(),
        "hi"
    );
    assert!(bool::from_arg(Some(&Value::Bool(true))).unwrap());
    assert_eq!("hi".into_value(), Value::Str("hi".into()));
    assert_eq!(().into_value(), Value::Nil);
}

#[test]
fn node_and_callback_handles_survive_a_round_trip() {
    assert_eq!(NodeId::from_arg(Some(&Value::Node(9))).unwrap().0, 9);
    assert_eq!(NodeId(9).into_value(), Value::Node(9));
    assert_eq!(
        CallbackId::from_arg(Some(&Value::Callback(CallbackId(3))))
            .unwrap()
            .0,
        3
    );
}

/// A wrong type must name what it wanted and what it got. This is the message
/// a script author sees, so it is part of the API.
#[test]
fn a_wrong_type_says_what_it_expected() {
    let err = bool::from_arg(Some(&Value::Str("no".into())))
        .unwrap_err()
        .to_string();
    assert!(err.contains("bool"), "does not name the wanted type: {err}");
    assert!(
        err.contains("string"),
        "does not name the given type: {err}"
    );
}

#[test]
fn a_missing_argument_is_an_error_not_a_default() {
    assert!(i64::from_arg(None).is_err());
    assert!(String::from_arg(None).is_err());
    assert!(bool::from_arg(None).is_err());
}

/// `Value` itself accepts anything, which is how a binding takes an options
/// table without naming its shape.
#[test]
fn the_neutral_value_accepts_anything() {
    for v in [
        Value::Nil,
        Value::Int(1),
        Value::Map(vec![]),
        Value::List(vec![]),
    ] {
        assert_eq!(Value::from_arg(Some(&v)).unwrap(), v);
    }
}

#[test]
fn tuples_map_positionally() {
    let args = [Value::Int(1), Value::Str("two".into()), Value::Bool(true)];
    let (a, b, c) = <(i64, String, bool)>::from_args(&args).unwrap();
    assert_eq!((a, b.as_str(), c), (1, "two", true));
}

/// Extra arguments are ignored, the way Lua itself ignores them. A binding
/// declared `()` accepts a stray argument rather than erroring, so this is
/// pinned as the contract rather than left to be discovered.
#[test]
fn extra_arguments_are_ignored_like_lua_does() {
    assert!(<()>::from_args(&[]).is_ok());
    assert!(<()>::from_args(&[Value::Int(1)]).is_ok());
    let (a,) = <(i64,)>::from_args(&[Value::Int(1), Value::Int(2)]).unwrap();
    assert_eq!(a, 1);
}

/// A trailing optional may be left off, which is how `set_component(name)` and
/// `set_component(name, params)` are the same binding.
#[test]
fn a_trailing_optional_may_be_omitted() {
    assert_eq!(<Option<i64>>::from_args(&[]).unwrap(), None);
    assert_eq!(<Option<i64>>::from_args(&[Value::Int(5)]).unwrap(), Some(5));
    let (name, params) = <(String, Option<i64>)>::from_args(&[Value::Str("a".into())]).unwrap();
    assert_eq!((name.as_str(), params), ("a", None));
}

#[test]
fn too_few_arguments_for_a_tuple_is_an_error() {
    assert!(<(i64, i64)>::from_args(&[Value::Int(1)]).is_err());
}

#[test]
fn expect_arity_names_the_function_and_both_counts() {
    assert!(expect_arity(&[Value::Int(1)], 1, "f").is_ok());
    let err = expect_arity(&[], 2, "spawn").unwrap_err().to_string();
    assert!(
        err.contains("spawn") && err.contains('2'),
        "unhelpful: {err}"
    );
}

#[test]
fn vectors_and_colors_carry_their_components() {
    assert_eq!(Value::Vec2([1.0, 2.0]), Value::Vec2([1.0, 2.0]));
    assert_ne!(Value::Vec3([1.0, 2.0, 3.0]), Value::Vec3([1.0, 2.0, 4.0]));
    assert_ne!(Value::Vec2([1.0, 2.0]), Value::Vec3([1.0, 2.0, 0.0]));
}

/// An app with no script backend still builds its plugins; their registrations
/// go nowhere rather than panicking.
#[test]
fn no_bindings_accepts_registrations_and_discards_them() {
    struct Host;
    let mut sink = NoBindings;
    let m: &mut dyn Bindings<Host> = &mut sink;
    m.function("f", |_: &Host, ()| Ok(1i64));
    m.constant("K", Value::Int(1));
    m.function_raw("raw", Box::new(|_, _| Ok(Value::Nil)));
}

/// `&mut T` and `Box<T>` forward, which is how a plugin can take either.
#[test]
fn bindings_forward_through_references_and_boxes() {
    struct Host;
    let mut boxed: Box<dyn Bindings<Host>> = Box::new(NoBindings);
    boxed.function("via_box", |_: &Host, ()| Ok(()));
    let by_ref: &mut dyn Bindings<Host> = &mut *boxed;
    by_ref.function("via_ref", |_: &Host, ()| Ok(()));
}
