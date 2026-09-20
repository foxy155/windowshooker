//! SigilScript abstract syntax tree.
//!
//! Every node the parser can produce and the interpreter can walk.
//! No logic lives here — this is a pure data module so both the parser
//! and the interpreter can depend on it without cycles.

use std::fmt;

// ============================================================
// SOURCE LOCATIONS
// ============================================================

/// A single position in the source. Line is 1-based, col is 1-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(line: u32, col: u32) -> Self {
        Self { line, col }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// A node with its source span attached. Used by anything the
/// interpreter wants to point at when it errors.
#[derive(Debug, Clone)]
pub struct Node<T> {
    pub span: Span,
    pub node: T,
}

impl<T> Node<T> {
    pub fn new(span: Span, node: T) -> Self {
        Self { span, node }
    }
}

pub type Expr = Node<ExprKind>;
pub type Stmt = Node<StmtKind>;
pub type TypeNode = Node<TypeKind>;

// ============================================================
// TYPES
// ============================================================

#[derive(Debug, Clone)]
pub enum TypeKind {
    // Primitives
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float,
    Float32,
    Float64,
    Bool,
    Bool8,
    String,
    CString,
    WString,
    Bytes,
    Ptr,
    Void,
    Null,

    /// A user-defined or imported type by name.
    Named(String),

    /// `vec<T>`
    Vec(Box<TypeNode>),

    /// `map<K, V>`
    Map(Box<TypeNode>, Box<TypeNode>),

    /// `set<T>`
    Set(Box<TypeNode>),

    /// `option<T>`
    Option(Box<TypeNode>),

    /// `result<T, E>`
    Result(Box<TypeNode>, Box<TypeNode>),

    /// A tuple type: `(A, B, C)`.
    Tuple(Vec<TypeNode>),

    /// A function type: `A -> B -> C`.
    Fn(Vec<TypeNode>, Box<TypeNode>),

    /// An inferred placeholder, e.g. `_`.
    Infer,
}

// ============================================================
// EXPRESSIONS
// ============================================================

#[derive(Debug, Clone)]
pub enum ExprKind {
    // ---- Literals ----
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    RawStr(String),
    ByteStr(Vec<u8>),
    Char(char),
    Null,

    // ---- Names ----
    Ident(String),

    // ---- Operators ----
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    /// `a ? b : c` is not legal syntax; we use `b if a else c`.
    /// This node is the if-expression form.
    IfExpr {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },

    // ---- Calls ----
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },

    /// Method call: `receiver.method(args)`.
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
    },

    /// Generic method: `receiver.method::<T>(args)`.
    GenericMethodCall {
        receiver: Box<Expr>,
        method: String,
        type_args: Vec<TypeNode>,
        args: Vec<CallArg>,
    },

    // ---- Field and index access ----
    Field {
        receiver: Box<Expr>,
        name: String,
    },

    /// Tuple field access: `t.0`, `t.1`.
    TupleField {
        receiver: Box<Expr>,
        index: usize,
    },

    Index {
        receiver: Box<Expr>,
        index: Box<Expr>,
    },

    /// Slicing: `data[0..10]`.
    Slice {
        receiver: Box<Expr>,
        range: SliceRange,
    },

    /// Optional chain: `a?.b`.
    OptionalField {
        receiver: Box<Expr>,
        name: String,
    },

    // ---- Collections ----
    VecLiteral(Vec<Expr>),

    MapLiteral(Vec<(Expr, Expr)>),

    SetLiteral(Vec<Expr>),

    TupleLiteral(Vec<Expr>),

    /// Struct literal: `Point { x: 1, y: 2 }`.
    StructLiteral {
        name: String,
        type_args: Vec<TypeNode>,
        fields: Vec<(String, Expr)>,
    },

    // ---- Ranges ----
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },

    // ---- Casting and type-checking ----
    Cast {
        expr: Box<Expr>,
        ty: TypeNode,
    },

    /// `value is Type`.
    IsType {
        expr: Box<Expr>,
        ty: TypeNode,
    },

    /// `sizeof::<T>()` and `offset_of::<T>("field")`.
    SizeOf(TypeNode),
    OffsetOf {
        ty: TypeNode,
        field: String,
    },
    TypeOf(Box<Expr>),

    // ---- Special operators ----
    /// Null coalescing: `a ?? b`.
    Coalesce {
        left: Box<Expr>,
        right: Box<Expr>,
    },

    /// Error propagation: `expr?`.
    TryPropagate(Box<Expr>),

    // ---- Anonymous / lambda ----
    /// A lambda: `|x| x + 1` or `|| { return 1 }`.
    Lambda {
        params: Vec<Param>,
        body: Box<Block>,
    },

    /// Block expression: `{ let x = 1; x + 1 }`.
    BlockExpr(Box<Block>),

    // ---- Match expression ----
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },

    // ---- Result and option constructors ----
    Some(Box<Expr>),
    NoneLit,
    Ok(Box<Expr>),
    Err(Box<Expr>),

    // ---- Try-expression: `try { ... }` in expression position ----
    TryExpr(Box<Block>),
}

#[derive(Debug, Clone)]
pub struct CallArg {
    /// Optional named argument: `foo(x: 1, y: 2)`.
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct SliceRange {
    pub start: Option<Box<Expr>>,
    pub end: Option<Box<Expr>>,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Deref,
    Ref,
    RefMut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // Range
    RangeExclusive,
    RangeInclusive,
}

// ============================================================
// STATEMENTS
// ============================================================

#[derive(Debug, Clone)]
pub enum StmtKind {
    /// `let x = expr` or `let mut x: T = expr`.
    Let {
        name: String,
        mutable: bool,
        ty: Option<TypeNode>,
        value: Option<Expr>,
    },

    /// `const NAME = expr`.
    Const {
        name: String,
        ty: Option<TypeNode>,
        value: Expr,
    },

    /// `addr name = expr : T`.
    AddrDecl {
        name: String,
        address: Expr,
        ty: TypeNode,
    },

    /// Bare expression used as a statement.
    Expr(Expr),

    /// Assignment: `lhs = rhs`.
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },

    /// `if` / `elif` / `else`.
    If {
        cond: Expr,
        then_block: Block,
        elifs: Vec<(Expr, Block)>,
        else_block: Option<Block>,
    },

    /// `while cond: ...`.
    While {
        cond: Expr,
        body: Block,
    },

    /// `loop: ...` (infinite) or `loop every Nms: ...`.
    Loop {
        interval_ms: Option<Expr>,
        body: Block,
    },

    /// `for name in iterable: ...`.
    For {
        name: String,
        iterable: Expr,
        body: Block,
    },

    /// `match subject: ...`.
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
    },

    /// `break`, `continue`, `return expr`, `throw expr`.
    Break,
    Continue,
    Return(Option<Expr>),
    Throw(Expr),

    /// `defer { ... }`.
    Defer(Block),

    /// `try { ... } catch err { ... } finally { ... }`.
    Try {
        body: Block,
        catch: Option<(String, Block)>,
        finally: Option<Block>,
    },

    /// `func name(params) -> T: body`.
    FuncDecl(FuncDecl),

    /// `struct Name { fields, methods }`.
    StructDecl(StructDecl),

    /// `enum Name { variants }`.
    EnumDecl(EnumDecl),

    /// `impl Type { methods }`.
    ImplDecl(ImplDecl),

    /// `type Name = T`.
    TypeAlias {
        name: String,
        generics: Vec<String>,
        ty: TypeNode,
    },

    /// `script name: body` — declares the whole file's entry point.
    ScriptDecl {
        name: String,
        attrs: Vec<Attribute>,
        body: Block,
    },

    /// `import "path"` or `import { ... } from "path"` or `import "path" as alias`.
    Import(ImportDecl),

    /// `export name` / `pub name`.
    Export(String),

    /// `breakpoint("label")`.
    Breakpoint(String),

    /// `assert(cond, msg)`.
    Assert {
        cond: Expr,
        message: Option<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    BitAndAssign,
    BitOrAssign,
    BitXorAssign,
    ShlAssign,
    ShrAssign,
    UshrAssign,
}

// ============================================================
// BLOCKS AND DECLARATIONS
// ============================================================

#[derive(Debug, Clone)]
pub struct Block {
    pub span: Span,
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeNode>,
    pub default: Option<Expr>,
    pub variadic: bool,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Block,
    pub attrs: Vec<Attribute>,
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,
    /// Optional trait bound: `T: Printable`.
    pub bound: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub attrs: Vec<Attribute>,
    pub fields: Vec<StructField>,
    pub methods: Vec<FuncDecl>,
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: TypeNode,
    pub default: Option<Expr>,
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub attrs: Vec<Attribute>,
    pub variants: Vec<EnumVariant>,
    pub public: bool,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    /// None = unit variant. Some(vec![]) = tuple variant with fields.
    /// Some(named) = struct variant with named fields.
    pub payload: EnumPayload,
}

#[derive(Debug, Clone)]
pub enum EnumPayload {
    Unit,
    Tuple(Vec<TypeNode>),
    Struct(Vec<(String, TypeNode)>),
}

#[derive(Debug, Clone)]
pub struct ImplDecl {
    pub type_name: String,
    pub generics: Vec<GenericParam>,
    pub methods: Vec<FuncDecl>,
}

// ============================================================
// PATTERNS
// ============================================================

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `_`
    Wildcard,

    /// `name` — binds the matched value.
    Binding(String),

    /// Literal: `42`, `"hi"`, `true`.
    Literal(Literal),

    /// `A..B` or `A..=B`.
    Range {
        start: Box<Pattern>,
        end: Box<Pattern>,
        inclusive: bool,
    },

    /// `a | b | c`.
    Or(Vec<Pattern>),

    /// Tuple: `(a, b, c)`.
    Tuple(Vec<Pattern>),

    /// Slice: `[first, ..rest]`.
    Slice {
        patterns: Vec<Pattern>,
        rest: Option<Box<Pattern>>,
    },

    /// Struct: `Point { x: 0, y }`.
    Struct {
        name: String,
        fields: Vec<(String, Pattern)>,
        rest: bool,
    },

    /// Tuple variant: `Some(x)` or `Color::RGB(r, g, b)`.
    TupleVariant {
        path: Vec<String>,
        patterns: Vec<Pattern>,
    },

    /// Struct variant: `Damage::Poison { potency, duration }`.
    StructVariant {
        path: Vec<String>,
        fields: Vec<(String, Pattern)>,
        rest: bool,
    },
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Char(char),
    Null,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    /// Guard: `n if n > 1000 => ...`.
    pub guard: Option<Expr>,
    pub body: Expr,
}

// ============================================================
// ATTRIBUTES AND IMPORTS
// ============================================================

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<AttributeArg>,
}

#[derive(Debug, Clone)]
pub enum AttributeArg {
    /// Positional: `#[version("1.0")]`.
    Positional(Expr),
    /// Named: `#[name(foo = "bar")]`.
    Named(String, Expr),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: String,
    pub alias: Option<String>,
    /// For `import { a, b } from "path"`.
    pub items: Vec<String>,
}

// ============================================================
// WHOLE FILE
// ============================================================

/// A parsed source file. Contains every top-level statement and the
/// span of the file as a whole. This is what `parser::parse` returns.
#[derive(Debug, Clone)]
pub struct Program {
    pub span: Span,
    pub statements: Vec<Stmt>,
}

// ============================================================
// HELPERS
// ============================================================

impl ExprKind {
    pub fn as_int(&self) -> Option<i64> {
        if let ExprKind::Int(n) = self { Some(*n) } else { None }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ExprKind::Str(s) | ExprKind::RawStr(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl BinaryOp {
    /// Precedence level, higher binds tighter. Used by the parser.
    pub fn precedence(&self) -> u8 {
        match self {
            BinaryOp::RangeInclusive | BinaryOp::RangeExclusive => 1,
            BinaryOp::Or => 2,
            BinaryOp::And => 3,
            BinaryOp::Eq | BinaryOp::Ne => 4,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 5,
            BinaryOp::BitOr => 6,
            BinaryOp::BitXor => 7,
            BinaryOp::BitAnd => 8,
            BinaryOp::Shl | BinaryOp::Shr | BinaryOp::Ushr => 9,
            BinaryOp::Add | BinaryOp::Sub => 10,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 11,
            BinaryOp::Pow => 12,
        }
    }

    pub fn is_left_associative(&self) -> bool {
        // Exponentiation is right-associative: 2**3**2 == 2**(3**2)
        !matches!(self, BinaryOp::Pow)
    }
}

impl AssignOp {
    pub fn is_compound(&self) -> bool {
        !matches!(self, AssignOp::Assign)
    }
}