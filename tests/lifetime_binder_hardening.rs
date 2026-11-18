//! Adversarial lifetime binder preflight, without granting reference ABI rights.
//! A recognized generic head must NEVER be stripped and lowered as a non-generic
//! declaration. These tests validate rejection diagnostics and existing guests.
use atto_compiler::{lexer, parser::Parser};

fn parse_error(source: &str) -> &'static str {
    let tokens = match lexer::lex(source) {
        Ok(t) => t,
        Err(e) => return e.code,
    };
    match Parser::new(tokens).parse_program() {
        Ok(_) => panic!("unsupported lifetime generic unexpectedly parsed: {source}"),
        Err(e) => e.code,
    }
}

fn head(params: &str) -> String {
    format!("fnc identity<{params}>(): int {{ return 42; }}")
}

#[test]
fn compact_long_and_non_ascii_binders_are_recognized_but_not_lowered() {
    for binder in [
        "lt!a", "lifetime!a", "lt! a", "lifetime! a",
        "lt!\na", "lt! /* name */ a", "lt! α",
        "T, lt! a", "lt! a, T", "lt! a, lifetime! b",
    ] {
        let src = head(binder);
        assert_eq!(parse_error(&src), "E_UNSUPPORTED_TYPE", "{binder}");
        assert_eq!(atto_compiler::compile(&src).unwrap_err().code,
            "E_UNSUPPORTED_TYPE", "{binder}");
    }
}

#[test]
fn binders_in_all_nominal_declaration_heads_fail_closed() {
    for src in [
        "define class Boxed<lt! a> as end",
        "define enum Tag<lt! a> as Present end",
        "define actor Worker<lt! a> as end",
        "define class Boxed as fnc f<lt! a>(): int { return 42; } end",
        "define class Boxed as pub fnc f<lifetime! a>(): int { return 42; } end",
    ] {
        assert_eq!(parse_error(src), "E_UNSUPPORTED_TYPE", "{src}");
        assert_eq!(atto_compiler::compile(src).unwrap_err().code,
            "E_UNSUPPORTED_TYPE", "{src}");
    }
}

#[test]
fn no_whitespace_or_comments_between_lifetime_keyword_and_bang() {
    for marker in [
        "lt ! a", "lifetime ! a", "lt\n! a", "lt\t!a",
        "lt/**/!a", "lt/*a*/! a", "lifetime/**/!a",
        "lt = a", "lt!=a", "lifetime=a",
        "lt a", "lifetime a", "lt!", "lifetime!",
    ] {
        assert_eq!(parse_error(&head(marker)), "E_SYNTAX", "{marker}");
    }
}

#[test]
fn duplicates_and_reserved_lifetime_names_are_rejected() {
    for binder in [
        "lt! a, lt! a", "lifetime! a, lt! a",
        "lt! a, T, lifetime! a", "lt! a, lifetime! b, lt! b",
    ] {
        assert_eq!(parse_error(&head(binder)), "E_DUPLICATE", "{binder}");
    }
    for binder in ["lt! self", "lt! true", "lifetime! return"] {
        assert_eq!(parse_error(&head(binder)), "E_RESERVED", "{binder}");
    }
}

#[test]
fn parameter_limit_counts_lifetimes_and_types_together() {
    let at_limit = (0..32).map(|i| format!("lt! l{i}"))
        .collect::<Vec<_>>().join(", ");
    assert_eq!(parse_error(&head(&at_limit)), "E_UNSUPPORTED_TYPE");
    for overflowing in [
        format!("{at_limit}, T"),
        (0..33).map(|i| format!("T{i}")).collect::<Vec<_>>().join(", "),
        (0..33).map(|i| format!("lt! l{i}")).collect::<Vec<_>>().join(", "),
    ] {
        assert_eq!(parse_error(&head(&overflowing)), "E_SOURCE_LIMIT");
    }
}

#[test]
fn no_binder_can_erase_already_parsed_program_or_emit_guest() {
    for prefix in [
        "pub fnc main(): int { return 42; }",
        "define class Existing as pub let int value = 12; end",
    ] {
        for binder in ["lt! a", "lifetime! b", "T, lt! c"] {
            let src = format!("{prefix}\n{}", head(binder));
            let err = atto_compiler::compile(&src).expect_err("hidden lifetime cannot be erased");
            assert_eq!(err.code, "E_UNSUPPORTED_TYPE", "{src}");
        }
    }
}

#[test]
fn lifetime_names_are_not_runtime_operations_or_ownership_permissions() {
    for source in [
        "lt! a;",
        "pub fnc main(): int { val x = lt! a; return 42; }",
        "pub fnc main(): int { val x = lifetime! a; return 42; }",
        "pub fnc main(): int { val x: Borrow<int, lt=a> = 42; return x; }",
        "pub fnc main(): int { val x: BorrowMut<int, lt=a> = 42; return 42; }",
    ] {
        assert!(atto_compiler::compile(source).is_err(), "unsafe runtime syntax admitted: {source}");
    }
    // Contextual marker recognition must not reserve ordinary local names.
    for ident in ["lt", "lifetime"] {
        let program = format!("pub fnc main(): int {{ val {ident} = 42; return {ident}; }}");
        assert!(atto_compiler::compile(&program).unwrap().starts_with(b"\0asm"));
    }
}

#[test]
fn malformed_generic_head_fuzz_is_bounded_and_cannot_become_guest_bytecode() {
    // Deterministic corpus with punctuation, syntactic noise, comments,
    // keyword/bang separations, and Unicode; avoid host entropy.
    let atoms = [
        "lt", "lifetime", "!", "a", "b", "T", ",", ">",
        "<", "=", "(", ")", "/*x*/", "\n", " ", "α", "'a'",
    ];
    let mut seed = 0xA77A_2026u64;
    for case in 0..800 {
        let mut params = String::new();
        for _ in 0..(case % 13 + 1) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            params.push_str(atoms[(seed as usize) % atoms.len()]);
        }
        let src = head(&params);
        // An unsupported generic function must either fail the lexer
        // or fail the parser; it must never be accepted as a guest.
        let result = std::panic::catch_unwind(|| atto_compiler::compile(&src));
        assert!(result.is_ok(), "panic in case {case}: {params:?}");
        assert!(result.unwrap().is_err(), "generic admitted in case {case}: {params:?}");
    }
}
