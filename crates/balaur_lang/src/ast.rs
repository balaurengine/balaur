#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pos {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Node,
    Void,
    Array(Box<Ty>),
    Struct(String),
    /// A value of any kind, inspected when the script runs.
    Any,
}

impl Ty {
    pub(crate) fn is_heap(&self) -> bool {
        matches!(self, Ty::Str | Ty::Array(_) | Ty::Any)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub(crate) enum Part {
    Text(String),
    Value(Expr),
}

#[derive(Clone, Debug)]
pub(crate) enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Interp(Vec<Part>),
    Var(String),
    SelfRef,
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Call(String, Vec<Expr>),
    Method(Box<Expr>, String, Vec<Expr>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    StructLit(String, Vec<(String, Expr)>),
    ArrayLit(Vec<Expr>),
    ArrayRepeat(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Block, Option<Block>),
    Cast(Box<Expr>, Ty),
    Await(Box<Expr>),
    /// The value a resumed `await` hands back, by await site; produced by the async lowering only.
    Resumed(Ty, u32),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub pos: Pos,
}

#[derive(Clone, Debug)]
pub(crate) enum StmtKind {
    Let {
        name: String,
        ty: Option<Ty>,
        init: Expr,
    },
    Assign {
        target: Expr,
        op: Option<BinOp>,
        value: Expr,
    },
    Expr(Expr),
    While {
        cond: Expr,
        body: Block,
    },
    Loop(Block),
    ForRange {
        var: String,
        from: Expr,
        to: Expr,
        body: Block,
    },
    ForEach {
        var: String,
        items: Expr,
        body: Block,
    },
    Return(Option<Expr>),
    Break,
    Continue,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub name: String,
    pub ty: Ty,
}

/// A struct field; `pub` with a default is what a scene may tune.
#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub name: String,
    pub ty: Ty,
    pub public: bool,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub(crate) struct FnDecl {
    pub name: String,
    pub owner: Option<String>,
    pub public: bool,
    pub is_async: bool,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub body: Block,
    pub pos: Pos,
}

impl FnDecl {
    pub(crate) fn key(&self) -> String {
        match &self.owner {
            Some(o) => format!("{o}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ExternDecl {
    pub name: String,
    pub is_async: bool,
    pub params: Vec<Param>,
    pub ret: Ty,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Program {
    pub structs: Vec<(String, Vec<Field>)>,
    pub fns: Vec<FnDecl>,
    pub externs: Vec<ExternDecl>,
    pub consts: Vec<(String, Expr)>,
}
