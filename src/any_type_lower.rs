//! Checked monomorphic scalar-Any specialization for single-source browser guests.
//!
//! This is **not** general runtime Any: no dynamic boxes, runtime type tags,
//! imported dispatch, capture, cross-scope mutation, or polymorphic function
//! pointers. A source-local immutable Any literal makes its scalar type
//! statically provable. One direct function accepting one Any parameter may
//! specialize to that same scalar. The ordinary parser/checker/Wasm emitter
//! then enforces type-correct results and guard control flow.
//!
//! Type arms whose tag cannot match the proven scalar are removed; compatible
//! arms lower to guards / catch-alls preserving arm order and once-only match
//! subject evaluation. Unsupported Any shapes fail *before* emitting Wasm.
use std::collections::BTreeMap;

use crate::diag::{Diagnostic, Pos};
use crate::lexer::{Tok, Token};

type R<T> = Result<T, Diagnostic>;
const MAX_TYPE_ARMS: usize = 64;

fn word(t: &[Token], i: usize, w: &str) -> bool {
    matches!(t.get(i).map(|x| &x.tok), Some(Tok::Ident(s)) if s == w)
}
fn fail(p: Pos, code: &'static str, description: impl Into<String>) -> Diagnostic {
    Diagnostic::new(p, code, description)
}
fn scalar_lit(t: &Tok) -> Option<&'static str> {
    match t {
        Tok::Int(Some(_)) => Some("int"),
        Tok::Str(_) => Some("string"),
        Tok::Float(_) => Some("float"),
        Tok::Ident(s) if matches!(s.as_str(), "true" | "false") => Some("bool"),
        _ => None,
    }
}
fn scalar_type(s: &str) -> Option<&'static str> {
    match s {
        "int" => Some("int"),
        "bool" | "boolean" => Some("bool"),
        "string" | "String" => Some("string"),
        "float" | "f64" => Some("float"),
        _ => None,
    }
}

fn replace_match_arms(t: &[Token], start: usize, concrete: &str) -> R<(Vec<Token>, usize)> {
    // Exactly 'match known_scalar over on ... -> { ... } ... end'.
    // Braced bodies are self-delimiting; nested scopes are not scanned for
    // misleading 'on' / 'end' identifiers.
    let pos = t[start].pos;
    if start + 2 >= t.len() || !word(t, start + 2, "over") {
        return Err(fail(pos, "E_UNSUPPORTED",
            "monomorphic Any matching requires 'match known_value over'"));
    }
    let mut j = start + 3;
    let mut arms = 0;
    let mut selected = false;
    let mut output = t[start..start + 3].to_vec();
    while !word(t, j, "end") {
        let Some(head) = t.get(j) else {
            return Err(fail(pos, "E_EOF", "unterminated statically checked Any match"));
        };
        if head.tok == Tok::Eof {
            return Err(fail(pos, "E_EOF", "unterminated statically checked Any match"));
        }
        if !word(t, j, "on") {
            return Err(fail(head.pos, "E_UNSUPPORTED",
                "monomorphic Any match admits only on-is-Type or on-_ arms"));
        }
        arms += 1;
        if arms > MAX_TYPE_ARMS {
            return Err(fail(head.pos, "E_SOURCE_LIMIT", "Any match has too many type arms"));
        }
        let typed = word(t, j + 1, "is");
        let wildcard = word(t, j + 1, "_");
        if !typed && !wildcard {
            return Err(fail(head.pos, "E_UNSUPPORTED",
                "Any match requires a type pattern ('on is int') or wildcard"));
        }
        let arm_ty = if typed {
            let ty = match t.get(j + 2).map(|x| &x.tok) {
                Some(Tok::Ident(s)) => scalar_type(s),
                _ => None,
            };
            Some(ty.ok_or_else(|| fail(head.pos, "E_UNSUPPORTED_TYPE",
                "runtime object type patterns are not implemented; only provable scalar tags are supported"))?)
        } else { None };
        let header_end = j + if typed { 3 } else { 2 };
        let mut body_open = header_end;
        while body_open < t.len() && t[body_open].tok != Tok::LBrace {
            if t[body_open].tok == Tok::Eof || word(t, body_open, "on") ||
                word(t, body_open, "end") {
                return Err(fail(t[body_open].pos, "E_SYNTAX", "Any type match arm requires a braced body"));
            }
            body_open += 1;
        }
        if body_open == t.len() || body_open == header_end || t[body_open - 1].tok != Tok::Arrow {
            return Err(fail(head.pos, "E_SYNTAX", "Any type match requires '-> { ... }'"));
        }
        let mut depth = 1usize;
        let mut end = body_open + 1;
        while depth > 0 {
            let Some(x) = t.get(end) else {
                return Err(fail(head.pos, "E_EOF", "unclosed Any match arm body"));
            };
            match x.tok {
                Tok::LBrace => depth += 1,
                Tok::RBrace => depth -= 1,
                Tok::Eof => return Err(fail(head.pos, "E_EOF", "unclosed Any match arm body")),
                _ => {}
            }
            if depth > 128 { return Err(fail(head.pos, "E_NESTING", "type arm nesting exceeds 128")); }
            end += 1;
        }
        let compatible = arm_ty.is_none_or(|ty| ty == concrete);
        let guarded = word(t, header_end, "when");
        if compatible && !selected {
            output.push(t[j].clone()); // on
            if typed {
                output.push(Token { tok: Tok::Ident("_".into()), pos: t[j + 1].pos });
                output.extend_from_slice(&t[j + 3..end]);
            } else {
                output.extend_from_slice(&t[j + 1..end]);
            }
            if !guarded { selected = true; }
        }
        j = end;
    }
    if arms == 0 {
        return Err(fail(pos, "E_MATCH", "Any match requires at least one arm"));
    }
    if output.len() == 3 {
        return Err(fail(pos, "E_MATCH", "no type arm can match the statically known Any tag"));
    }
    if !selected {
        return Err(fail(pos, "E_MATCH",
            "statically checked Any type match must have an unguarded matching or wildcard arm"));
    }
    output.push(t[j].clone()); // end
    Ok((output, j + 1))
}

/// Reject dynamic, escaped and inconsistent Any values rather than silently
/// treating unrelated runtime types as one scalar. The regular checker
/// validates all rewritten direct calls, actual expressions and branch code.
pub fn lower(mut t: Vec<Token>) -> R<Vec<Token>> {
    if !t.iter().any(|x| word(std::slice::from_ref(x), 0, "Any")) {
        return Ok(t);
    }
    let mut any_locals = BTreeMap::<String, &'static str>::new();
    let mut any_params = Vec::<(usize, String, String)>::new();
    let mut replacement = Vec::<(usize, &'static str)>::new();
    for i in 0..t.len() {
        if !word(&t, i, "Any") { continue; }
        // Exact, immutable, literal-evaluable binding: val x: Any = 42;
        if i >= 3 && word(&t, i - 3, "val") &&
            matches!(t[i - 2].tok, Tok::Ident(_)) &&
            t[i - 1].tok == Tok::Colon && t.get(i + 1).is_some_and(|x| x.tok == Tok::Assign)
        {
            let type_name = t.get(i + 2).and_then(|x| scalar_lit(&x.tok))
                .ok_or_else(|| fail(t[i].pos, "E_UNSUPPORTED_TYPE",
                    "Any literal specialization requires a direct immutable scalar initializer"))?;
            if t.get(i + 3).is_none_or(|x| x.tok != Tok::Semi) {
                return Err(fail(t[i].pos, "E_UNSUPPORTED_TYPE",
                    "Any literal specialization does not accept computed or escaped values"));
            }
            let Tok::Ident(name) = &t[i - 2].tok else { unreachable!() };
            if any_locals.insert(name.clone(), type_name).is_some() {
                return Err(fail(t[i].pos, "E_DUPLICATE",
                    "multiple Any bindings of the same name require lexical type analysis"));
            }
            replacement.push((i, type_name));
            continue;
        }
        // A single monomorphic top-level fnc(Any param) supported by one
        // statically proven immutable Any literal binding at its call site.
        if i >= 3 && word(&t, i - 3, "fnc") &&
            matches!(t[i - 2].tok, Tok::Ident(_)) &&
            t[i - 1].tok == Tok::LParen &&
            matches!(t.get(i + 1).map(|x| &x.tok), Some(Tok::Ident(_))) &&
            t.get(i + 2).is_some_and(|x| x.tok == Tok::RParen)
        {
            let Tok::Ident(name) = &t[i - 2].tok else { unreachable!() };
            let Tok::Ident(param) = &t[i + 1].tok else { unreachable!() };
            any_params.push((i, name.clone(), param.clone()));
            continue;
        }
        return Err(fail(t[i].pos, "E_UNSUPPORTED_TYPE",
            "general Any values, dynamic type tags and polymorphic runtime dispatch require a checked Any ABI"));
    }
    if any_params.len() != 1 || any_locals.len() != 1 {
        return Err(fail(t[0].pos, "E_UNSUPPORTED",
            "bounded Any specialization needs exactly one immutable literal and one direct function accepting Any"));
    }
    let (index, fn_name, param_name) = &any_params[0];
    let (local_name, concrete) = any_locals.iter().next().unwrap();
    if param_name == local_name {
        // The two scopes are independent but both must be specialized to the
        // same concrete type. A second unrelated declaration of this name
        // would require full lexical shadow analysis, so reject it.
        let count = t.iter().enumerate().filter(|(i,x)|
            word(std::slice::from_ref(x), 0, local_name) &&
            (*i != index + 1) &&
            !(*i >= 1 && word(&t, i - 1, "val"))
        ).count();
        if count > 24 {
            return Err(fail(t[*index].pos, "E_SOURCE_LIMIT", "Any specialization use budget exceeded"));
        }
    }
    if t.iter().enumerate().any(|(i,_)| word(&t,i,"fnc") &&
        i+2 < t.len() && word(&t,i+1,fn_name) && i+2!=index-1) {
        return Err(fail(t[*index].pos, "E_DUPLICATE", "monomorphic Any function is overloaded"));
    }
    for i in 0..t.len() {
        if word(&t,i,fn_name) && t.get(i+1).is_some_and(|x| x.tok==Tok::LParen)
            && i != index-2 {
            if !word(&t,i+2,local_name) || t.get(i+3).is_none_or(|x| x.tok!=Tok::RParen) {
                return Err(fail(t[i].pos, "E_UNSUPPORTED",
                    "Any monomorphization only supports direct calls using the proven immutable binding"));
            }
        }
    }
    replacement.push((*index, concrete));
    for (i, typ) in replacement { t[i].tok = Tok::Ident(typ.to_owned()); }
    let mut tags = BTreeMap::new();
    tags.insert(param_name.clone(), *concrete);
    tags.insert(local_name.clone(), *concrete);
    let mut out = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if word(&t, i, "match") &&
            t.get(i+1).is_some_and(|x| matches!(x.tok, Tok::Ident(_))) &&
            t.get(i+2).is_some_and(|x| word(std::slice::from_ref(x),0,"over")) &&
            matches!(t.get(i+1).map(|x| &x.tok), Some(Tok::Ident(name)) if tags.contains_key(name)) {
            let Tok::Ident(name) = &t[i+1].tok else { unreachable!() };
            let (expanded,next) = replace_match_arms(&t,i,tags[name])?;
            out.extend(expanded);
            i=next;
            continue;
        }
        if word(&t,i,"is") && i > 0 && matches!(t[i-1].tok,Tok::Ident(_)) {
            let Tok::Ident(name) = &t[i-1].tok else { unreachable!() };
            if let Some(actual) = tags.get(name) {
                let test = match t.get(i+1).map(|x| &x.tok) {
                    Some(Tok::Ident(type_name)) => scalar_type(type_name),
                    _ => None
                }.ok_or_else(|| fail(t[i].pos,"E_UNSUPPORTED_TYPE",
                    "monomorphic Any 'is' requires a supported scalar type"))?;
                if out.last().is_none_or(|x: &Token| x.tok != t[i-1].tok) {
                    return Err(fail(t[i].pos,"E_SYNTAX","'is' requires the checked scalar operand"));
                }
                out.pop();
                out.push(Token {tok:Tok::Ident(if *actual==test {"true"} else {"false"}.into()),pos:t[i-1].pos});
                i+=2;
                continue;
            }
            return Err(fail(t[i].pos,"E_UNSUPPORTED","'is' on a value without a proven Any type tag is not executable"));
        }
        out.push(t[i].clone());
        i+=1;
    }
    if out.len()>crate::lexer::MAX_TOKENS {
        return Err(fail(t[0].pos,"E_SOURCE_LIMIT","Any specialization exceeds lexer token budget"));
    }
    Ok(out)
}
