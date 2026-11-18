//! The canonical checked-handle prefixes and their legacy rt spellings
//! must produce identical parse trees. Until the backend has a verified
//! checked-handle MIR/runtime, ALL of them must fail closed before codegen.
use atto_compiler::{
    ast::{Expr, ExprKind, OwnershipOp, Stmt},
    lexer, parser::Parser,
};

fn expr(source: &str) -> Expr {
    let src = format!("pub fnc main(): int {{ val result = {source}; return 0; }}");
    let p = Parser::new(lexer::lex(&src).expect("lex"))
        .parse_program().expect("parse canonical checked-handle syntax");
    let Stmt::Bind { init, .. } = &p.funcs[0].body[0] else {
        panic!("expected binding");
    };
    init.clone()
}

fn checked_op(source: &str, expected: OwnershipOp) -> ExprKind {
    let actual = expr(source);
    let ExprKind::Ownership(op, mutable, inner) = actual.kind else {
        panic!("expected an ownership or checked-handle AST node: {source}");
    };
    assert_eq!(op, expected, "{source}");
    assert!(!mutable, "{source}: checked handles cannot be mutable borrows");
    inner.kind
}

#[test]
fn canonical_and_legacy_handle_prefixes_have_the_same_ast() {
    for (modern, compact, grouped, legacy, action) in [
        ("ref! h", "ref!h", "ref!(h)", "rt ref h", OwnershipOp::Ref),
        ("deref! h", "deref!h", "deref!(h)", "rt deref h", OwnershipOp::Deref),
        ("unref! h", "unref!h", "unref!(h)", "rt unref h", OwnershipOp::Unref),
    ] {
        let expected = format!("{:?}", checked_op(modern, action));
        assert_eq!(expected, format!("{:?}", checked_op(compact, action)));
        assert_eq!(expected, format!("{:?}", checked_op(grouped, action)));
        assert_eq!(expected, format!("{:?}", checked_op(legacy, action)));
        assert_eq!(expected, "Var(\"h\")");
    }
}

#[test]
fn postfix_operands_preserve_exact_structure_not_a_function_call() {
    let member = checked_op("deref! h.value", OwnershipOp::Deref);
    assert!(matches!(member,
        ExprKind::Member(base, field) if field == "value"
            && matches!(base.kind, ExprKind::Var(ref name) if name == "h")));

    let nested = checked_op("ref! (deref! h)", OwnershipOp::Ref);
    assert!(matches!(nested, ExprKind::Ownership(OwnershipOp::Deref, false, _)));
}

#[test]
fn unsupported_handle_abi_is_rejected_for_bang_and_legacy_spellings() {
    for form in [
        "ref! h", "ref!h", "ref!(h)", "rt ref h",
        "deref! h", "deref!h", "deref!(h)", "rt deref h",
        "unref! h", "unref!h", "unref!(h)", "rt unref h",
    ] {
        let src = format!("pub fnc main(): int {{ val h = 42; val x = {form}; return 0; }}");
        let e = atto_compiler::compile(&src).unwrap_err();
        assert_eq!(e.code, "E_UNSUPPORTED_HANDLE", "{form}: {e:?}");
    }
}

#[test]
fn ref_syntax_rejects_separated_marker_and_mut_qualifier() {
    for illegal in ["ref !h", "deref !h", "unref !h",
                    "ref! mut h", "deref! mut h", "unref! mut h",
                    "rt ref mut h", "rt deref mut h", "rt unref mut h"] {
        let src = format!("pub fnc main(): int {{ val h = 42; val x = {illegal}; return 0; }}");
        let parsed = Parser::new(lexer::lex(&src).expect("lex")).parse_program();
        assert!(parsed.is_err(), "invalid checked-handle syntax accepted: {illegal}");
    }
}

#[test]
fn similar_identifiers_without_attached_bang_remain_ordinary_calls() {
    let src = r#"
        fnc ref(int h): int { return h; }
        fnc deref(int h): int { return h; }
        fnc unref(int h): int { return h; }
        pub fnc main(): int { return ref(deref(unref(42))); }
    "#;
    let wasm = atto_compiler::compile(src).expect("ordinary identifiers are not handle prefixes");
    assert!(wasm.starts_with(b"\0asm"));
}

#[test]
fn mixed_rt_and_bang_notation_is_not_silently_accepted() {
    for illegal in ["rt ref! h", "rt deref! h", "rt unref! h"] {
        let src = format!("pub fnc main(): int {{ val h = 42; val x = {illegal}; return 0; }}");
        let err = Parser::new(lexer::lex(&src).expect("lex")).parse_program().unwrap_err();
        assert_eq!(err.code, "E_SYNTAX", "wrong mixed-syntax error for {illegal}");
    }
}

#[test]
fn checked_handles_nested_in_unreachable_paths_and_lambdas_are_rejected_pre_lowering() {
    for source in [
        "pub fnc main(): int { if false { val x = ref! 42; } fi return 42; }",
        "pub fnc main(): int { val callback_value = |x| -> deref! x; return 42; }",
        "pub fnc main(): int { val h = true ? 42 : unref! 42; return 42; }",
        "pub fnc main(): int { val a = [1, 2, ref! 3]; return 42; }",
        "pub fnc main(): int { return 42; } fnc hidden(): int { return ref! 4; }",
    ] {
        let err = atto_compiler::compile(source).unwrap_err();
        assert_eq!(err.code, "E_UNSUPPORTED_HANDLE",
            "a nested checked handle must not be dropped/rewritten: {source}; {err:?}");
    }
}

#[test]
fn checked_handle_gate_checks_all_declaration_containers() {
    use atto_compiler::ast::{ActorDecl, ClassDecl, Field, FieldMode, Method, Receive,
        Program, Expr, ExprKind, Stmt, OwnershipOp, Type};
    use atto_compiler::check;
    let pos = atto_compiler::diag::Pos { line: 14, col: 8 };
    let make_handle = || Expr {
        pos,
        kind: ExprKind::Ownership(OwnershipOp::Ref, false, Box::new(Expr {
            pos,
            kind: ExprKind::Int(Some(42)),
        })),
    };
    let make_field = || Field {
        name: "slot".into(), pos, ty: Type::Int, public: false,
        mode: FieldMode::Final, init: make_handle(),
    };
    let make_method = || Method {
        name: "get".into(), pos, public: false, is_static: false,
        params: Vec::new(), ret: Type::Int,
        body: vec![Stmt::Return(Some(make_handle()), pos)],
    };
    let mut program = Program::default();
    program.classes.push(ClassDecl {
        name: "Container".into(), pos, public: false, state_container: false,
        fields: vec![make_field()], ctor: None, methods: Vec::new(),
    });
    assert_eq!(check::check(&program).unwrap_err().code, "E_UNSUPPORTED_HANDLE");

    program.classes[0].fields.clear();
    program.classes[0].methods.push(make_method());
    assert_eq!(check::check(&program).unwrap_err().code, "E_UNSUPPORTED_HANDLE");

    program.classes.clear();
    program.actors.push(ActorDecl {
        name: "Worker".into(), pos,
        fields: vec![make_field()], receive: None, methods: Vec::new(),
    });
    assert_eq!(check::check(&program).unwrap_err().code, "E_UNSUPPORTED_HANDLE");

    program.actors[0].fields.clear();
    program.actors[0].receive = Some(Receive {
        pos, mail: "msg".into(), payload: Type::Int,
        body: vec![Stmt::Expr(make_handle())],
    });
    assert_eq!(check::check(&program).unwrap_err().code, "E_UNSUPPORTED_HANDLE");

    program.actors[0].receive = None;
    program.actors[0].methods.push(make_method());
    assert_eq!(check::check(&program).unwrap_err().code, "E_UNSUPPORTED_HANDLE");
}

#[test]
fn checked_handle_preflight_has_finite_depth_for_untrusted_ast_builders() {
    use atto_compiler::ast::{Expr, ExprKind, Func, Program, Stmt, Type, UnOp};
    use atto_compiler::check;
    let pos = atto_compiler::diag::Pos { line: 1, col: 1 };
    let mut value = Expr { kind: ExprKind::Int(Some(42)), pos };
    for _ in 0..140 {
        value = Expr { kind: ExprKind::Unary(UnOp::Plus, Box::new(value)), pos };
    }
    let mut program = Program::default();
    program.funcs.push(Func {
        name: "main".into(), pos, is_generator: false, is_pure: false,
        params: Vec::new(), ret: Type::Int,
        body: vec![Stmt::Return(Some(value), pos)],
    });
    assert_eq!(check::check(&program).unwrap_err().code, "E_NESTING");

    // A chain of standalone blocks must hit the same limit without
    // recursing first through diagnostic-position discovery.
    let mut block = Stmt::Return(Some(Expr {
        kind: ExprKind::Int(Some(42)), pos,
    }), pos);
    for _ in 0..140 {
        block = Stmt::Block(vec![block]);
    }
    program.funcs[0].body = vec![block];
    assert_eq!(check::check(&program).unwrap_err().code, "E_NESTING");
}
