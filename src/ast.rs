//! Untyped syntax tree produced by the parser.

use crate::diag::Pos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Bool,
    Void,
    /// `float` / `f64`: IEEE-754 binary64 (Java `double`).
    Float,
    /// `string` / `String`: immutable UTF-8 text, a view into the module's
    /// string data (no heap).
    Str,
    /// Unresolved user type name: index into [`Program::type_names`].
    /// The checker replaces it with `Class` or `Enum`.
    Named(u32),
    /// Unresolved `ActorRef<Name>`: index into [`Program::type_names`].
    NamedRef(u32),
    /// Source-level `fnc(T1, T2) -> R` signature. The index refers to a
    /// canonical, compiler-interned signature in `Program::type_names`.
    /// This type is recognized and checked, but cannot yet be represented
    /// as a runtime value or Wasm function-table entry.
    Callable(u32),
    /// Instance of `Program::classes[i]` (a heap reference).
    Class(u32),
    /// Value of `Program::enums[i]`.
    Enum(u32),
    /// `ActorRef<A>` for `Program::actors[i]`: a copyable mailbox capability.
    ActorRef(u32),
    /// First executable ADT profiles; their payloads use the checked module heap.
    /// Other generic instantiations reject without emitting an artifact.
    OptionInt,
    ResultIntStr,
    /// First nested ADT profile: Some(Result<int,string>) or None.
    /// Generic ADT instantiations remain deliberately restricted.
    OptionResultIntStr,
    /// Bounded, owned guest-Wasm generator handle for the admitted subset.
    GeneratorInt,
}

impl Type {
    /// Name of a scalar type. User types are named by the checker, which
    /// knows the declaration tables.
    pub fn name(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Bool => "bool",
            Type::Void => "void",
            Type::Float => "float",
            Type::Str => "string",
            Type::OptionInt => "Option<int>",
            Type::ResultIntStr => "Result<int,string>",
            Type::OptionResultIntStr => "Option<Result<int,string>>",
            Type::GeneratorInt => "Generator<int>",
            Type::Callable(_) => "fnc(...)->...",
            _ => "object",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub classes: Vec<ClassDecl>,
    pub enums: Vec<EnumDecl>,
    pub actors: Vec<ActorDecl>,
    /// Optional non-executable out-of-line actor interface in the production AST.
    /// Executable actor code still uses the normal actors field above.
    pub actor_interface: Option<ActorInterface>,
    /// Spellings referenced by `Type::Named` / `Type::NamedRef`, plus
    /// canonical compiler-interned source callable signatures.
    pub type_names: Vec<(String, Pos)>,
}


/// Target-only nominal actor interface retained in the production Program AST.
/// Indices in method ActorRef types refer to forwards, NOT executable actors.
/// The backend must not emit this partial interface.
#[derive(Clone, Debug)]
pub struct ActorInterface {
    pub namespace: Option<String>,
    pub forwards: Vec<String>,
    pub methods: Vec<ActorInterfaceMethod>,
}

/// Typed out-of-line receiver signature; its body is not lowered.
#[derive(Clone, Debug)]
pub struct ActorInterfaceMethod {
    pub owner: u32,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Type,
    pub mutable: bool,
    pub public: bool,
    pub pos: Pos,
}

/// Who may assign a field after its initializer ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMode {
    /// `name: T = e`, `final ...`, `val`/`const` fields: never reassigned.
    Final,
    /// Class `T name: e`, `mut T name = e`, `let T name = e`: the constructor
    /// may assign it; methods see a read-only `self` (Java ownership rule).
    Constructor,
    /// Actor-owned state, or a protected state-container field whose writes
    /// require the typed, checked critical-write scope.
    ActorState,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub pos: Pos,
    pub ty: Type,
    pub public: bool,
    pub mode: FieldMode,
    pub init: Expr,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pub pos: Pos,
    pub public: bool,
    pub is_static: bool,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Constructor {
    pub pos: Pos,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

/// `[pub] define class Name as { member } end` (grammar.ebnf `class_decl`).
#[derive(Clone, Debug)]
pub struct ClassDecl {
    pub name: String,
    pub pos: Pos,
    pub public: bool,
    /// Distinct nominal protected-state declaration, never an ordinary class.
    /// Only a restricted Worker-local scalar backend has executable lowering.
    pub state_container: bool,
    pub fields: Vec<Field>,
    pub ctor: Option<Constructor>,
    pub methods: Vec<Method>,
}

/// `define enum Name as Variant ... end`: the oreslang-source.llvm v1 design
/// contract spelling (contracts/v1/design/enum_pattern_cases.json). The Java
/// reference parser does not admit it yet.
#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub name: String,
    pub pos: Pos,
    pub variants: Vec<(String, Pos)>,
}

/// `receive(ActorMail<T> mail): void { ... }`.
#[derive(Clone, Debug)]
pub struct Receive {
    pub pos: Pos,
    pub mail: String,
    pub payload: Type,
    pub body: Vec<Stmt>,
}

/// `define actor Name as ... end` or `actor Name ... end` (Java Parser).
#[derive(Clone, Debug)]
pub struct ActorDecl {
    pub name: String,
    pub pos: Pos,
    pub fields: Vec<Field>,
    pub receive: Option<Receive>,
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub name: String,
    pub pos: Pos,
    /// Explicit `fnc *gen` modifier; not inferred from `yield`.
    pub is_generator: bool,
    /// Explicit effect contract: no externally observable effects or impure calls.
    pub is_pure: bool,
    pub params: Vec<Param>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub pos: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindKind {
    /// `val`
    Val,
    /// `const`: initializer must be a compile-time constant
    Const,
    /// `let`
    Let,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Bind {
        kind: BindKind,
        name: String,
        ty: Option<Type>,
        init: Expr,
        pos: Pos,
    },
    Assign {
        name: String,
        value: Expr,
        pos: Pos,
    },
    /// Lexical scope admission for protected state writes, retained through
    /// the typed frontend so a class field cannot manufacture write authority.
    StateWrite {
        body: Vec<Stmt>,
        pos: Pos,
    },
    /// `target.field = value`; only `self.field` is admitted by the checker.
    FieldAssign {
        target: Expr,
        field: String,
        value: Expr,
        pos: Pos,
    },
    Expr(Expr),
    /// Ordinary guest failure. The checker admits only throw Error(string).
    /// Wasm's fatal traps, panics and host exceptions are never this signal.
    Throw(Expr, Pos),
    /// Escalated failure (`raise Error(..)`): bypasses ordinary `catch`
    /// and `trap`; only a registered `recover` or the root boundary sees it.
    Raise(Expr, Pos),
    /// `try { } [catch (Error e) { }] [finally { }]` (defer-recover-unwind-v1).
    Try {
        body: Vec<Stmt>,
        /// Binder name and handler of the single admitted `catch (Error e)`.
        catch: Option<(String, Vec<Stmt>)>,
        finally: Option<Vec<Stmt>>,
        pos: Pos,
    },
    /// `defer || -> { ... };` registers a LIFO cleanup without running it.
    Defer { body: Vec<Stmt>, pos: Pos },
    /// `recover (|T a, ..| -> |err, c| -> { ... })(args);` registers a
    /// recovery callback whose factory arguments are evaluated once, now.
    Recover {
        params: Vec<(String, Type, Pos)>,
        err: String,
        cursor: String,
        body: Vec<Stmt>,
        args: Vec<Expr>,
        pos: Pos,
    },
    Yield(Expr, Pos),
    YieldFrom(Expr, Pos),
    Return(Option<Expr>, Pos),
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Option<Vec<Stmt>>,
        pos: Pos,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        pos: Pos,
    },
    Loop {
        body: Vec<Stmt>,
        pos: Pos,
    },
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        update: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        pos: Pos,
    },
    /// Pull-driven `for value in generator do ... done` iteration.
    ForIn {
        name: String,
        source: Expr,
        body: Vec<Stmt>,
        pos: Pos,
    },
    Break(Pos),
    Continue(Pos),
    Block(Vec<Stmt>),
    /// `match subject over on P -> { } ... end` (canonical, source-ordered)
    /// or the legacy `match [first] subject P -> { } ... end`.
    Match {
        subject: Expr,
        arms: Vec<MatchArm>,
        pos: Pos,
    },
}

#[derive(Clone, Debug)]
pub enum Pattern {
    /// `_`, or `else` in the legacy form
    Wild,
    Int(i64),
    Bool(bool),
    /// lower-case name: binds the subject
    Bind(String),
    /// `Enum.Variant` (oreslang-source.llvm v1 enum design spelling)
    Variant(String, String),
    /// Tagged built-in ADT variant, optionally binding the payload.
    Adt(String, Option<String>),
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `,` inside an if/while condition: eager, left-to-right AND
    And,
    /// `|` inside an if/while condition: eager, left-to-right OR
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Eq => "eq",
            BinOp::Ne => "neq",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => ",",
            BinOp::Or => "|",
        }
    }
}

/// Lambda bodies distinguish an implicit-return expression, a single
/// terminated statement, and a braced multi-statement block.
#[derive(Clone, Debug)]
pub enum LambdaBody {
    Expression(Box<Expr>),
    Statement(Box<Stmt>),
    Block(Vec<Stmt>),
}

/// Explicit Atto ownership and checked-handle operators, never raw
/// pointer sigils. Ref/Deref/Unref require a verified checked-handle IR
/// and must be rejected by backends without that runtime contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnershipOp { Borrow, Copy, Take, Share, Ref, Deref, Unref }

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Integer literal. `None` is 2^63, only legal as the operand of unary minus.
    Int(Option<u64>),
    /// float literal, as `f64::to_bits`
    Float(u64),
    Bool(bool),
    Str(String),
    Var(String),
    Unary(UnOp, Box<Expr>),
    /// Unary call-site trap: evaluates its operand once and wraps only
    /// an ordinary guest throw into None, preserving all successful values.
    Trap(Box<Expr>),
    /// Prefix ownership/checked handles: borrow!, copy!, take!, share!,
    /// ref!, deref!, unref!. Parentheses group the operand, not a call.
    /// rt ref/rt deref/rt unref are compatibility forms, same AST kind.
    Ownership(OwnershipOp, bool, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Postfix `?`: unwrap Ok/Some or propagate Err/None from the caller.
    Propagate(Box<Expr>),
    /// `name(args)` or the qualified builtins `stdio.print` / `stdio.println`.
    Call(String, Vec<Expr>),
    /// `self`
    SelfRef,
    /// Compiler-recognized immutable by-value projection from an explicit
    /// `handle_state(critical-read |s| -> s.field)` scope. Never ordinary
    /// class member access: the checker must prove a protected state lease.
    StateRead(String),
    /// `new Class(args)`
    New(String, Vec<Expr>),
    /// `spawn Actor()`
    Spawn(String),
    /// `base.name`: field read, `Enum.Variant`, or `mail.value`.
    Member(Box<Expr>, String),
    /// `base.name(args)`: method, `Class.static(args)` or `ref.send(v)`.
    MethodCall(Box<Expr>, String, Vec<Expr>),
    /// Syntax-only sequence literal. Browser guest allocation is not lowered.
    Array(Vec<Expr>),
    /// Parsed lambda signature/body. The browser backend deliberately rejects
    /// callable values until closure capture and runtime types are implemented.
    Lambda {
        params: Vec<String>,
        return_type: Option<String>,
        body: LambdaBody,
    },
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub pos: Pos,
}
