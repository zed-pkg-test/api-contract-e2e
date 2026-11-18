//! Checked source-local scalar product shapes for the browser Wasm compiler.
//!
//! This lowering uses the existing typed **class field / constructor / guest
//! object** implementation, never compile-time simulated stdout. Struct and
//! record member declarations become real nominal classes with constructor
//! initialization and checked property reads. Two-field Tuple/Pair/Entry are
//! distinct internal nominal products with ordered scalar fields.
//!
//! Scope: direct immutable scalar product construction. It is NOT arbitrary
//! structural subtyping, collections, generic runtime reflection, actor
//! transport or value-copy ABI parity. Dynamic/unknown product shapes reject.
use std::collections::{BTreeMap, BTreeSet};

use crate::diag::{Diagnostic, Pos};
use crate::lexer::{self, Tok, Token};

type R<T> = Result<T, Diagnostic>;
pub const PREFIX: &str = "__atto_shape_";
const MAX_SHAPES: usize = 64;
const MAX_FIELDS: usize = 8;
const MAX_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind { Struct, Record }
#[derive(Clone)]
struct Field { name: String, ty: String }
#[derive(Clone)]
struct Decl { name: String, kind: Kind, fields: Vec<Field>, pos: Pos }

fn word(tokens: &[Token], i: usize, s: &str) -> bool {
    matches!(tokens.get(i).map(|t| &t.tok), Some(Tok::Ident(w)) if w == s)
}
fn error(pos: Pos, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(pos, code, message)
}
fn id(tokens: &[Token], i: usize, what: &str) -> R<String> {
    let t = tokens.get(i).ok_or_else(|| error(Pos {line:1,col:1}, "E_EOF",
        format!("missing {what}")))?;
    match &t.tok {
        Tok::Ident(name) if !name.is_empty() && name.len() <= MAX_NAME_BYTES &&
            !matches!(name.as_str(), "as" | "end" | "define" | "new" | "infer" | "struct" | "record" | "pub") =>
            Ok(name.clone()),
        _ => Err(error(t.pos, "E_SYNTAX", format!("invalid {what}"))),
    }
}
fn scalar(word: &str) -> Option<&'static str> {
    match word {
        "int" => Some("int"),
        "string" | "String" => Some("string"),
        "bool" | "boolean" => Some("bool"),
        "float" | "f64" => Some("float"),
        _ => None,
    }
}
fn default(ty: &str) -> &'static str {
    match ty {
        "int" => "0", "bool" => "false", "string" => r#""""#, "float" => "0.0",
        _ => unreachable!("scalar() checked the field"),
    }
}
fn fragment(src: &str, pos: Pos) -> R<Vec<Token>> {
    let mut out = lexer::lex(src)?;
    if out.last().is_some_and(|x| x.tok == Tok::Eof) { out.pop(); }
    for t in &mut out { t.pos = pos; }
    Ok(out)
}
fn synthesized(klass: &str, fields: &[Field]) -> String {
    let mut src = format!("define class {klass} as\n");
    for f in fields {
        src.push_str(&format!("pub {} {}: {};\n", f.ty, f.name, default(&f.ty)));
    }
    src.push_str("constructor(");
    for (i, f) in fields.iter().enumerate() {
        if i > 0 { src.push_str(", "); }
        src.push_str(&format!("{} {}", f.ty, f.name));
    }
    src.push_str(") {\n");
    for f in fields {
        src.push_str(&format!("self.{} = {};\n", f.name, f.name));
    }
    src.push_str("}\nend\n");
    src
}

fn parse_decl(t: &[Token], i: usize, kind: Kind) -> R<(Decl, usize)> {
    let pos = t[i].pos;
    let name = id(t, i+2, "product name")?;
    if !word(t, i+3, "as") {
        return Err(error(pos, "E_SYNTAX", "product declaration requires 'define struct/record Name as'"));
    }
    let mut fields = Vec::new();
    let mut j = i+4;
    while !word(t, j, "end") {
        let current = t.get(j).ok_or_else(|| error(pos, "E_EOF","unterminated product declaration"))?;
        if current.tok == Tok::Eof {
            return Err(error(pos, "E_EOF", "unterminated product declaration"));
        }
        if fields.len() >= MAX_FIELDS {
            return Err(error(current.pos, "E_SOURCE_LIMIT", "product shape has too many fields"));
        }
        let ty = match &current.tok {
            Tok::Ident(w) => scalar(w),
            _ => None,
        }.ok_or_else(|| error(current.pos, "E_UNSUPPORTED_TYPE",
            "product field must be a scalar int/string/bool/float"))?;
        let field = id(t,j+1,"product field")?;
        if t.get(j+2).is_none_or(|x| x.tok != Tok::Semi) {
            return Err(error(current.pos,"E_UNSUPPORTED",
                "product declarations admit only semicolon-terminated scalar fields"));
        }
        if fields.iter().any(|f: &Field| f.name == field) {
            return Err(error(current.pos,"E_DUPLICATE",format!("duplicate field '{field}'")));
        }
        fields.push(Field { name: field,ty:ty.to_owned() });
        j+=3;
    }
    if fields.is_empty() {return Err(error(pos,"E_UNSUPPORTED","empty product shapes are not in the executable profile"));}
    Ok((Decl{name,kind,fields,pos}, j+1))
}

fn simple_literal(tok: &Tok) -> bool {
    matches!(tok, Tok::Int(Some(_)) | Tok::Float(_) | Tok::Str(_)) ||
        matches!(tok, Tok::Ident(s) if matches!(s.as_str(),"true"|"false"))
}
fn keyed(t: &[Token], open: usize) -> R<(BTreeMap<String, Token>, usize)> {
    if t.get(open).is_none_or(|x| x.tok != Tok::LBrace) {
        return Err(error(t[open.saturating_sub(1)].pos,"E_SYNTAX","product initializer requires '{'"));
    }
    let mut values=BTreeMap::new();
    let mut j=open+1;
    while t.get(j).is_some_and(|x| x.tok != Tok::RBrace) {
        if values.len() >= MAX_FIELDS {
            return Err(error(t[j].pos,"E_SOURCE_LIMIT","too many product initializer fields"));
        }
        let field=id(t,j,"initializer field")?;
        if t.get(j+1).is_none_or(|x| x.tok != Tok::Colon) {
            return Err(error(t[j].pos,"E_SYNTAX","product member initializer uses ':'"));
        }
        let value=t.get(j+2).ok_or_else(|| error(t[j].pos,"E_EOF","missing product value"))?;
        if !simple_literal(&value.tok) {
            return Err(error(value.pos,"E_UNSUPPORTED",
                "source-local keyed products currently require literal scalar fields"));
        }
        if values.insert(field.clone(),value.clone()).is_some() {
            return Err(error(t[j].pos,"E_DUPLICATE",format!("duplicate initialized field '{field}'")));
        }
        j+=3;
        if t.get(j).is_some_and(|x| x.tok == Tok::Comma) {j+=1;}
        else if t.get(j).is_none_or(|x| x.tok != Tok::RBrace) {
            return Err(error(value.pos,"E_SYNTAX","product fields require commas"));
        }
    }
    if t.get(j).is_none() { return Err(error(t[open].pos,"E_EOF","unterminated product literal")); }
    Ok((values,j+1))
}

fn constructor_tokens(decl: &Decl, values: BTreeMap<String,Token>, pos: Pos) -> R<Vec<Token>> {
    if values.len()!=decl.fields.len() ||
        decl.fields.iter().any(|f|!values.contains_key(&f.name)) {
        return Err(error(pos,"E_SHAPE",
            format!("'{}' constructor fields differ from its declared product shape",decl.name)));
    }
    let mut out=fragment(&format!("new {}(",decl.name),pos)?;
    for (i,f) in decl.fields.iter().enumerate() {
        if i>0 {out.push(Token{tok:Tok::Comma,pos});}
        out.push(values[&f.name].clone());
    }
    out.push(Token{tok:Tok::RParen,pos});
    Ok(out)
}
fn synthetic_name(kind: &str, a: &str, b: &str) -> String {
    format!("{PREFIX}{kind}_{a}_{b}")
}
fn generics(t: &[Token], index: usize) -> R<(String,String,usize)> {
    let pos=t[index].pos;
    if t.get(index+1).is_none_or(|x| x.tok != Tok::Lt) ||
        t.get(index+3).is_none_or(|x| x.tok != Tok::Comma) ||
        t.get(index+5).is_none_or(|x| x.tok != Tok::Gt) {
        return Err(error(pos,"E_UNSUPPORTED_TYPE",
            "source-local Tuple/Pair/Entry profile admits exactly two scalar type arguments"));
    }
    let a=match t.get(index+2).map(|x|&x.tok) {
        Some(Tok::Ident(s))=>scalar(s), _=>None
    }.ok_or_else(|| error(pos,"E_UNSUPPORTED_TYPE","first product type must be scalar"))?;
    let b=match t.get(index+4).map(|x|&x.tok) {
        Some(Tok::Ident(s))=>scalar(s), _=>None
    }.ok_or_else(|| error(pos,"E_UNSUPPORTED_TYPE","second product type must be scalar"))?;
    Ok((a.to_owned(),b.to_owned(),index+6))
}
fn closing(t:&[Token],open:usize) -> R<usize> {
    if t.get(open).is_none_or(|x| x.tok != Tok::LParen) {
        return Err(error(t[open.saturating_sub(1)].pos,"E_SYNTAX","expected '(' for product arguments"));
    }
    let mut level=0usize;
    for i in open..t.len() {
        match t[i].tok {
            Tok::LParen=>level+=1,
            Tok::RParen=>{
                level-=1;
                if level==0 {return Ok(i);}
            }
            Tok::Eof=>break,
            _=>{}
        }
        if level>128 {return Err(error(t[open].pos,"E_NESTING","nested product argument limit"));}
    }
    Err(error(t[open].pos,"E_EOF","unterminated positional product literal"))
}
fn typed_product(t:&[Token],i:usize,defs:&mut BTreeMap<String,Vec<Field>>)
    -> R<Option<(Vec<Token>,usize)>>
{
    if !word(t,i,"val") ||
        !matches!(t.get(i+1).map(|x|&x.tok),Some(Tok::Ident(_))) ||
        t.get(i+2).is_none_or(|x| x.tok != Tok::Colon) {
        return Ok(None);
    }
    let kind=match t.get(i+3).map(|x|&x.tok) {
        Some(Tok::Ident(s)) if matches!(s.as_str(),"Tuple"|"Pair"|"Entry")=>s.as_str(),
        _=>return Ok(None)
    };
    let (a,b,after)=generics(t,i+3)?;
    if t.get(after).is_none_or(|x| x.tok != Tok::Assign) {
        return Err(error(t[i].pos,"E_SYNTAX","typed product binding requires '='"));
    }
    let open=if kind=="Tuple" {
        after+1
    } else {
        if !word(t,after+1,kind) {
            return Err(error(t[i].pos,"E_TYPE","typed product constructor kind must match the annotation"));
        }
        after+2
    };
    let close=closing(t,open)?;
    if t.get(close+1).is_none_or(|x| x.tok != Tok::Semi) {
        return Err(error(t[close].pos,"E_SYNTAX","product value must end in a semicolon"));
    }
    let cls=synthetic_name(&kind.to_lowercase(),&a,&b);
    if defs.len()>=MAX_SHAPES && !defs.contains_key(&cls) {
        return Err(error(t[i].pos,"E_SOURCE_LIMIT","too many generic product specializations"));
    }
    let named=match kind {
        "Tuple"=>["first","second"],"Pair"=>["first","second"],_=>["key","value"]
    };
    defs.entry(cls.clone()).or_insert_with(||vec![
        Field{name:named[0].into(),ty:a.clone()},Field{name:named[1].into(),ty:b.clone()}
    ]);
    let mut out=t[i..i+3].to_vec();
    out.push(Token{tok:Tok::Ident(cls.clone()),pos:t[i+3].pos});
    out.push(t[after].clone()); // =
    out.push(Token{tok:Tok::Ident("new".into()),pos:t[open].pos});
    out.push(Token{tok:Tok::Ident(cls),pos:t[open].pos});
    out.extend_from_slice(&t[open..=close]);
    out.push(t[close+1].clone()); // ;
    Ok(Some((out,close+2)))
}

/// Lower exactly source-local immutable two-field scalar products through
/// checked class constructors, preserving real runtime allocations and field
/// reads. Not executable for arbitrary structural/collection operations.
pub fn lower(t: Vec<Token>) -> R<Vec<Token>> {
    for x in &t {
        if let Tok::Ident(name)=&x.tok {
            if name.starts_with(PREFIX) {
                return Err(error(x.pos,"E_RESERVED","compiler-owned shape class names cannot appear in source"));
            }
        }
    }
    if !t.iter().enumerate().any(|(i,_)|
        word(&t,i,"struct")||word(&t,i,"record")||
        (word(&t,i,"Tuple")||word(&t,i,"Pair")||word(&t,i,"Entry"))) {
        return Ok(t);
    }
    let mut declared=BTreeMap::<String,Decl>::new();
    let mut first=Vec::new();
    let mut i=0;
    while i<t.len() {
        if word(&t,i,"define") &&
            (word(&t,i+1,"struct")||word(&t,i+1,"record")) {
            let kind=if word(&t,i+1,"struct"){Kind::Struct}else{Kind::Record};
            let (decl,next)=parse_decl(&t,i,kind)?;
            if declared.len()>=MAX_SHAPES {
                return Err(error(decl.pos,"E_SOURCE_LIMIT","too many named product definitions"));
            }
            if declared.contains_key(&decl.name) {
                return Err(error(decl.pos,"E_DUPLICATE",format!("duplicate product '{}'",decl.name)));
            }
            first.extend(fragment(&synthesized(&decl.name,&decl.fields),decl.pos)?);
            declared.insert(decl.name.clone(),decl);
            i=next;continue;
        }
        first.push(t[i].clone());
        i+=1;
    }
    let mut synthetic=BTreeMap::<String,Vec<Field>>::new();
    let mut out=Vec::new();
    i=0;
    while i<first.len() {
        if let Some((product,next))=typed_product(&first,i,&mut synthetic)? {
            out.extend(product);i=next;continue;
        }
        if word(&first,i,"infer") && word(&first,i+1,"struct") {
            let (values,next)=keyed(&first,i+2)?;
            let mut compatible=declared.values().filter(|d| d.kind==Kind::Struct &&
                d.fields.len()==values.len() &&
                d.fields.iter().all(|f|values.contains_key(&f.name)));
            let found=compatible.next().ok_or_else(||error(first[i].pos,"E_SHAPE",
                "no declared struct has this exact set of fields"))?;
            if compatible.next().is_some() {
                return Err(error(first[i].pos,"E_AMBIGUOUS_MEMBER",
                    "anonymous struct matches multiple declared named shapes"));
            }
            out.extend(constructor_tokens(found,values,first[i].pos)?);
            i=next;continue;
        }
        if let Tok::Ident(name)=&first[i].tok {
            if let Some(decl)=declared.get(name) {
                if i+1<first.len() && first[i+1].tok==Tok::LBrace {
                    let (values,next)=keyed(&first,i+1)?;
                    out.extend(constructor_tokens(decl,values,first[i].pos)?);
                    i=next;continue;
                }
            }
        }
        out.push(first[i].clone());
        i+=1;
    }
    if !synthetic.is_empty() {
        if out.last().is_some_and(|x|x.tok==Tok::Eof) {out.pop();}
        let pos=t.first().map(|x|x.pos).unwrap_or(Pos{line:1,col:1});
        for (name,fields) in synthetic {
            out.extend(fragment(&synthesized(&name,&fields),pos)?);
        }
        out.push(Token{tok:Tok::Eof,pos});
    }
    if out.len()>lexer::MAX_TOKENS {
        return Err(error(t[0].pos,"E_SOURCE_LIMIT","checked product specialization exceeds token budget"));
    }
    Ok(out)
}
