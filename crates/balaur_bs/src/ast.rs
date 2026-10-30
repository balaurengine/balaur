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
    /// `T?`: a `T`, or nil. Its slots are the `T`'s, then whether it is there.
    Optional(Box<Ty>),
    /// `fn(A, B) -> R`: a function named elsewhere, called later; no captures.
    /// Parameters, result, and which parameters are `&mut`.
    Fn(Vec<Ty>, Box<Ty>, Vec<bool>),
    /// `{ str: V }` or `{ int: V }`: a key's value, built in. Only the parser and
    /// the linker see one: before code is made each is the class the prelude's
    /// map is made into for that pair (`crate::maps`).
    Map(Box<Ty>, Box<Ty>),
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
    /// `nil`: no value, as whatever `T?` the place it goes to wants.
    Nil,
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
    /// `{k: v, ..}`, typed by where it goes.
    MapLit(Vec<(Expr, Expr)>),
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
    /// `&mut T`: the caller's own value, which gets the callee's writes.
    pub by_ref: bool,
    pub pos: Pos,
}

/// A struct field; `pub` with a default is what a scene may tune.
#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub name: String,
    pub ty: Ty,
    pub public: bool,
    pub default: Option<Expr>,
    pub pos: Pos,
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
    pub pos: Pos,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Program {
    pub structs: Vec<(String, Vec<Field>)>,
    /// The structs declared `class`: one counted object on the heap, which
    /// every holder shares, rather than a value each holder copies.
    pub classes: Vec<String>,
    pub fns: Vec<FnDecl>,
    pub externs: Vec<ExternDecl>,
    pub consts: Vec<(String, Expr)>,
    /// Where each struct and const is declared, for an editor's go-to.
    pub decls: Vec<(String, Pos)>,
    /// `use name;`, in order.
    pub uses: Vec<(String, Pos)>,
    /// The modules linked in, by the name each is used as.
    pub linked: Vec<String>,
    /// A linked function's key, and the file it came from.
    pub origin: Vec<(String, String)>,
    /// A linked class's name, and the file that declares it.
    pub class_origin: Vec<(String, String)>,
    /// Functions compiled in only once a call reaches them: the prelude's.
    pub lazy: Vec<FnDecl>,
}
