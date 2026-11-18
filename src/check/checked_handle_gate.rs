//! Fail-closed admission gate for checked-handle operations.
//!
//! Runs on the *original* parsed AST, before direct-lambda conversion,
//! CFG planning, evaluation ordering, or codegen. The current browser
//! guest has no authenticated checked-handle runtime. Every occurrence
//! of ref!, deref!, unref! or the legacy rt spellings must therefore
//! reject uniformly, even in branches that later optimize away.
//!
//! This is not a handle implementation: it is a security boundary that
//! prevents any lowered path from silently treating an unverified handle
//! as a borrowed, copied, or owned value.
use crate::ast::{Expr, ExprKind, LambdaBody, OwnershipOp, Program, Stmt};
use crate::diag::{Diagnostic, Pos};
use super::R;

/// Bounded traversal, including manually built ASTs using check::check().
const MAX_VISITS: usize = 65_536;
const MAX_DEPTH: usize = 128;

struct Scan {
    visits: usize,
}

impl Scan {
    fn enter(&mut self, pos: Pos, depth: usize) -> R<()> {
        if depth >= MAX_DEPTH {
            return Err(Diagnostic::new(pos, "E_NESTING",
                "checked-handle admission walk exceeded its nesting limit"));
        }
        if self.visits >= MAX_VISITS {
            return Err(Diagnostic::new(pos, "E_SOURCE_LIMIT",
                "checked-handle admission walk exhausted its work budget"));
        }
        self.visits += 1;
        Ok(())
    }

    fn exprs(&mut self, values: &[Expr], depth: usize) -> R<()> {
        for value in values { self.expr(value, depth)?; }
        Ok(())
    }

    fn stmts(&mut self, statements: &[Stmt], depth: usize) -> R<()> {
        for statement in statements { self.stmt(statement, depth)?; }
        Ok(())
    }

    fn expr(&mut self, e: &Expr, depth: usize) -> R<()> {
        self.enter(e.pos, depth)?;
        let next = depth + 1;
        match &e.kind {
            ExprKind::Ownership(op @ (OwnershipOp::Ref | OwnershipOp::Deref | OwnershipOp::Unref), ..) => {
                let spelling = match op {
                    OwnershipOp::Ref => "ref!",
                    OwnershipOp::Deref => "deref!",
                    OwnershipOp::Unref => "unref!",
                    _ => unreachable!(),
                };
                Err(Diagnostic::new(e.pos, "E_UNSUPPORTED_HANDLE",
                    format!("{spelling} requires verified checked-handle IR, runtime authority, and all-path retirement; no guest code emitted")))
            }
            ExprKind::Unary(_, value) | ExprKind::Trap(value) |
            ExprKind::Ownership(_, _, value) |
            ExprKind::Propagate(value) | ExprKind::Member(value, _) =>
                self.expr(value, next),
            ExprKind::Binary(_, left, right) => {
                self.expr(left, next)?;
                self.expr(right, next)
            }
            ExprKind::Ternary(cond, yes, no) => {
                self.expr(cond, next)?;
                self.expr(yes, next)?;
                self.expr(no, next)
            }
            ExprKind::Call(_, args) | ExprKind::New(_, args) |
            ExprKind::Array(args) => self.exprs(args, next),
            ExprKind::MethodCall(receiver, _, args) => {
                self.expr(receiver, next)?;
                self.exprs(args, next)
            }
            ExprKind::Lambda { body, .. } => match body {
                LambdaBody::Expression(value) => self.expr(value, next),
                LambdaBody::Statement(stmt) => self.stmt(stmt, next),
                LambdaBody::Block(stmts) => self.stmts(stmts, next),
            },
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) |
            ExprKind::Str(_) | ExprKind::Var(_) | ExprKind::SelfRef |
            ExprKind::StateRead(_) | ExprKind::Spawn(_) => Ok(()),
        }
    }

    fn stmt(&mut self, stmt: &Stmt, depth: usize) -> R<()> {
        let next = depth + 1;
        // Standalone Stmt::Block/Expr have no separate source position;
        // nested expression visits retain the precise token location.
        let pos = match stmt {
            Stmt::Bind { pos, .. } | Stmt::Assign { pos, .. } |
            Stmt::FieldAssign { pos, .. } | Stmt::StateWrite { pos, .. } |
            Stmt::Try { pos, .. } | Stmt::Defer { pos, .. } |
            Stmt::Recover { pos, .. } | Stmt::If { pos, .. } |
            Stmt::While { pos, .. } | Stmt::Loop { pos, .. } |
            Stmt::For { pos, .. } | Stmt::ForIn { pos, .. } |
            Stmt::Match { pos, .. } | Stmt::Throw(_, pos) |
            Stmt::Raise(_, pos) | Stmt::Yield(_, pos) |
            Stmt::YieldFrom(_, pos) | Stmt::Return(_, pos) |
            Stmt::Break(pos) | Stmt::Continue(pos) => *pos,
            Stmt::Expr(e) => e.pos,
            Stmt::Block(_) => Pos { line: 1, col: 1 },
        };
        self.enter(pos, depth)?;
        match stmt {
            Stmt::Bind { init, .. } => self.expr(init, next),
            Stmt::Assign { value, .. } => self.expr(value, next),
            Stmt::FieldAssign { target, value, .. } => {
                self.expr(target, next)?;
                self.expr(value, next)
            }
            Stmt::Expr(e) | Stmt::Throw(e, _) | Stmt::Raise(e, _) |
            Stmt::Yield(e, _) | Stmt::YieldFrom(e, _) => self.expr(e, next),
            Stmt::Return(Some(e), _) => self.expr(e, next),
            Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) => Ok(()),
            Stmt::Block(body) | Stmt::Loop { body, .. } |
            Stmt::StateWrite { body, .. } | Stmt::Defer { body, .. } =>
                self.stmts(body, next),
            Stmt::Try { body, catch, finally, .. } => {
                self.stmts(body, next)?;
                if let Some((_, block)) = catch { self.stmts(block, next)?; }
                if let Some(block) = finally { self.stmts(block, next)?; }
                Ok(())
            }
            Stmt::Recover { args, body, .. } => {
                self.exprs(args, next)?;
                self.stmts(body, next)
            }
            Stmt::If { arms, otherwise, .. } => {
                for (condition, body) in arms {
                    self.expr(condition, next)?;
                    self.stmts(body, next)?;
                }
                if let Some(body) = otherwise { self.stmts(body, next)?; }
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond, next)?;
                self.stmts(body, next)
            }
            Stmt::For { init, cond, update, body, .. } => {
                if let Some(init) = init { self.stmt(init, next)?; }
                if let Some(cond) = cond { self.expr(cond, next)?; }
                if let Some(update) = update { self.stmt(update, next)?; }
                self.stmts(body, next)
            }
            Stmt::ForIn { source, body, .. } => {
                self.expr(source, next)?;
                self.stmts(body, next)
            }
            Stmt::Match { subject, arms, .. } => {
                self.expr(subject, next)?;
                for arm in arms {
                    if let Some(guard) = &arm.guard { self.expr(guard, next)?; }
                    self.stmts(&arm.body, next)?;
                }
                Ok(())
            }
        }
    }
}

pub(super) fn validate(program: &Program) -> R<()> {
    let mut scan = Scan { visits: 0 };
    // Exhaustive original-program scope coverage, including branches that
    // control-flow lowering might eliminate and declaration initializers.
    for function in &program.funcs {
        scan.stmts(&function.body, 0)?;
    }
    for class in &program.classes {
        for field in &class.fields { scan.expr(&field.init, 0)?; }
        if let Some(ctor) = &class.ctor { scan.stmts(&ctor.body, 0)?; }
        for method in &class.methods { scan.stmts(&method.body, 0)?; }
    }
    for actor in &program.actors {
        for field in &actor.fields { scan.expr(&field.init, 0)?; }
        if let Some(receive) = &actor.receive { scan.stmts(&receive.body, 0)?; }
        for method in &actor.methods { scan.stmts(&method.body, 0)?; }
    }
    Ok(())
}
