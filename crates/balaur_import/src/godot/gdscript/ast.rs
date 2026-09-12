//! The tree between GDScript and Rune. It keeps GDScript's shapes, not
//! Rune's: `match` is still a match here and becomes an `if` chain at emit,
//! because Rune has no or-patterns.

#[derive(Clone, Debug)]
pub(crate) enum Expr {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
    Nil,
    /// A bare identifier. What it turns into depends on the class's members,
    /// which only the emitter knows.
    Name(String),
    SelfRef,
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// GDScript's `then if cond else other`.
    Ternary {
        then: Box<Expr>,
        cond: Box<Expr>,
        other: Box<Expr>,
    },
    Array(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Lambda {
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    /// `$Ship/Mast`, kept as its path text.
    NodePath(String),
    /// `%Unique`, Godot's scene-unique lookup.
    Unique(String),
    Await(Box<Expr>),
    /// `x as Ship`: a type assertion with no run-time work here.
    Cast(Box<Expr>, String),
    /// `x is Ship`, and `not x is Ship` when negated.
    Is(Box<Expr>, String, bool),
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    /// `var x := 1`, and `const` when `fixed`.
    Var {
        name: String,
        value: Option<Expr>,
        /// The declared type, where one was written: the emitter needs it to
        /// decide whether `0` or `0.0` is the right empty value.
        hint: Option<String>,
    },
    Assign {
        target: Expr,
        op: &'static str,
        value: Expr,
    },
    Expr(Expr),
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        other: Option<Vec<Stmt>>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        name: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    /// A line the parser could not read. It is emitted as a comment with a
    /// marker, and the report counts it.
    Raw(String),
}

#[derive(Clone, Debug)]
pub(crate) struct MatchArm {
    /// The values this arm matches. `_` is the wildcard and parses as a bare
    /// name, which the emitter recognises.
    pub patterns: Vec<Expr>,
    pub body: Vec<Stmt>,
}
