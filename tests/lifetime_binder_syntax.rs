//! Canonical lifetime-binder lexical/syntactic preflight.
//! These tests do not claim executable Borrow<T> generic lowering.
use atto_compiler::{lexer, parser::Parser};

fn diagnostic(source: &str) -> &'static str {
    let tokens = lexer::lex(source).expect("lex source");
    Parser::new(tokens).parse_program().unwrap_err().code
}

#[test]
fn canonical_and_long_form_lifetime_binders_are_both_recognized() {
    for marker in ["lt!", "lifetime!"] {
        for decl in [
            format!("fnc identity<T, {marker} a>(x: Borrow<T, lt=a>): int {{ return 1; }}"),
            format!("define class View<{marker} a, T> as end"),
            format!("fnc choose<T, {marker} a, lt! b>(x: Borrow<T, lt=a>, y: Borrow<T, lt=b>): int {{ return 1; }}"),
        ] {
            assert_eq!(diagnostic(&decl), "E_UNSUPPORTED_TYPE",
                "syntax should recognize the lifetime binder, then fail closed: {decl}");
        }
    }
}

#[test]
fn malformed_or_ambiguous_binders_are_syntax_errors() {
    for generic in [
        "lt a", "lifetime a", "lt ! a", "lt\n! a",
        "lifetime !a", "lt!", "lt! a,", "lt! a, lt b",
    ] {
        let source = format!("fnc f<{generic}>(): int {{ return 42; }}");
        assert_eq!(diagnostic(&source), "E_SYNTAX",
            "malformed binder should be rejected: {generic}");
    }
}

#[test]
fn lifetime_binders_cannot_be_duplicated_under_alias_spellings() {
    for generic in [
        "lt! a, lt! a",
        "lifetime! a, lt! a",
        "lt! a, lifetime! a",
    ] {
        let source = format!("fnc f<{generic}>(): int {{ return 42; }}");
        assert_eq!(diagnostic(&source), "E_DUPLICATE",
            "duplicate lifetime should be rejected: {generic}");
    }
}

#[test]
fn generic_admission_is_not_bypassed_by_lifetimes() {
    assert_eq!(diagnostic("fnc f<T>(): int { return 42; }"), "E_UNSUPPORTED");
    assert_eq!(diagnostic("fnc f<lt! a>(): int { return 42; }"), "E_UNSUPPORTED_TYPE");
    let invalid = "pub fnc main(): int { return 42; } fnc hidden<lt! a>(): int { return 1; }";
    assert_eq!(atto_compiler::compile(invalid).unwrap_err().code, "E_UNSUPPORTED_TYPE");
}

#[test]
fn lifetime_binder_work_is_bounded() {
    let many = (0..40).map(|i| format!("lt! a{i}")).collect::<Vec<_>>().join(", ");
    let source = format!("fnc f<{many}>(): int {{ return 0; }}");
    assert_eq!(diagnostic(&source), "E_SOURCE_LIMIT");
}

#[test]
fn named_borrow_types_must_not_gain_runtime_authority_from_spelling() {
    // Actual Borrow<T> and lifetime-specific reference signatures are not yet
    // admitted. This preserves the pre-existing non-forgeability safety gate.
    for borrowed in ["Borrow<int>", "BorrowMut<int>", "Borrow<int, lt=a>"] {
        let source = format!("fnc f(x: {borrowed}): int {{ return 42; }}");
        assert_eq!(atto_compiler::compile(&source).unwrap_err().code,
            "E_UNSUPPORTED_TYPE", "must not lower {borrowed}");
    }
}
