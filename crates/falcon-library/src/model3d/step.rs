//! A reader for STEP physical files (ISO 10303-21, the text form IFC
//! files use): the header's schema name and the `DATA;` section's
//! instances, `#id = ENTITY(args);`, with nested lists, strings, enums,
//! `$`, `*`, references and typed values such as `IFCLABEL('x')`.
//!
//! The parser works on tokens with an explicit stack, so nesting costs no
//! recursion; lists deeper than [`MAX_DEPTH`] are refused. Line breaks and
//! comments may fall anywhere between tokens.

use std::collections::HashMap;

use super::{ParseError, Result};

/// How deeply lists may nest inside one instance.
pub const MAX_DEPTH: usize = 64;

/// One parameter of an instance.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `$`: no value.
    Null,
    /// `*`: derived from a supertype.
    Derived,
    Int(i64),
    Real(f64),
    Str(String),
    /// `.NAME.`, without the dots.
    Enum(String),
    /// `#id`.
    Ref(u64),
    /// `"hex"`.
    Binary(String),
    List(Vec<Value>),
    /// `NAME(args)` inside a parameter list.
    Typed(String, Vec<Value>),
}

impl Value {
    pub fn as_ref_id(&self) -> Option<u64> {
        match self {
            Value::Ref(r) => Some(*r),
            _ => None,
        }
    }

    /// A number, integer or real; a typed number such as
    /// `IFCLENGTHMEASURE(2.)` too.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Typed(_, v) if v.len() == 1 => v[0].as_f64(),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// A string, or a typed string such as `IFCLABEL('x')`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Typed(_, v) if v.len() == 1 => v[0].as_str(),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<&str> {
        match self {
            Value::Enum(s) => Some(s),
            _ => None,
        }
    }
}

/// One instance: its entity name in capitals and its parameters. A
/// complex instance (`#1=(A(..)B(..));`) is named `""` and holds its parts
/// as typed values.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub name: String,
    pub args: Vec<Value>,
}

impl Entity {
    pub fn arg(&self, k: usize) -> &Value {
        self.args.get(k).unwrap_or(&Value::Null)
    }
}

/// A whole file: the schema names from `FILE_SCHEMA` and the instances by
/// id.
#[derive(Debug, Default)]
pub struct StepFile {
    pub schemas: Vec<String>,
    pub entities: HashMap<u64, Entity>,
}

impl StepFile {
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// The instance a value refers to, if it is a reference to one.
    pub fn deref(&self, v: &Value) -> Option<&Entity> {
        self.get(v.as_ref_id()?)
    }

    /// The instance a value refers to when it is named `name`.
    pub fn deref_named(&self, v: &Value, name: &str) -> Option<&Entity> {
        self.deref(v).filter(|e| e.name == name)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open,
    Close,
    Comma,
    Semi,
    Eq,
    Dollar,
    Star,
    Int(i64),
    Real(f64),
    Str(String),
    Enum(String),
    Ref(u64),
    Binary(String),
    Keyword(String),
}

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

fn err(msg: &str) -> ParseError {
    ParseError(format!("IFC: {msg}"))
}

impl<'a> Lexer<'a> {
    fn skip_space(&mut self) -> Result<()> {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                match self.b[self.i + 2..].windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i += 2 + p + 2,
                    None => return Err(err("unterminated comment")),
                }
            } else {
                return Ok(());
            }
        }
    }

    /// The next token, `None` at the end of the input.
    fn next(&mut self) -> Result<Option<Tok>> {
        self.skip_space()?;
        let Some(&c) = self.b.get(self.i) else { return Ok(None) };
        let start = self.i;
        self.i += 1;
        let t = match c {
            b'(' => Tok::Open,
            b')' => Tok::Close,
            b',' => Tok::Comma,
            b';' => Tok::Semi,
            b'=' => Tok::Eq,
            b'$' => Tok::Dollar,
            b'*' => Tok::Star,
            b'#' => {
                let s = self.take_while(|c| c.is_ascii_digit());
                Tok::Ref(s.parse().map_err(|_| err("bad instance name"))?)
            }
            b'\'' => Tok::Str(self.string()?),
            b'"' => {
                let s = self.take_while(|c| c.is_ascii_hexdigit()).to_string();
                if self.b.get(self.i) != Some(&b'"') {
                    return Err(err("unterminated binary value"));
                }
                self.i += 1;
                Tok::Binary(s)
            }
            b'.' if self.b.get(self.i).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') => {
                let s = self.take_while(|c| c.is_ascii_alphanumeric() || c == b'_').to_ascii_uppercase();
                if self.b.get(self.i) != Some(&b'.') {
                    return Err(err("unterminated enumeration"));
                }
                self.i += 1;
                Tok::Enum(s)
            }
            b'+' | b'-' | b'0'..=b'9' | b'.' => {
                self.i = start;
                let s = self.take_while(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E'));
                if s.contains(['.', 'e', 'E']) {
                    Tok::Real(s.parse().map_err(|_| err("bad number"))?)
                } else {
                    Tok::Int(s.parse().map_err(|_| err("bad number"))?)
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c == b'!' => {
                self.i = start;
                let s = self.take_while(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'!');
                Tok::Keyword(s.to_ascii_uppercase())
            }
            _ => return Err(err("unexpected character")),
        };
        Ok(Some(t))
    }

    fn take_while(&mut self, f: impl Fn(u8) -> bool) -> &'a str {
        let start = self.i;
        while self.i < self.b.len() && f(self.b[self.i]) {
            self.i += 1;
        }
        // Only ASCII bytes pass the filters used here.
        std::str::from_utf8(&self.b[start..self.i]).unwrap_or("")
    }

    /// A quoted string after its opening quote; `''` is a quote.
    fn string(&mut self) -> Result<String> {
        let mut raw = Vec::new();
        loop {
            let Some(&c) = self.b.get(self.i) else { return Err(err("unterminated string")) };
            self.i += 1;
            if c == b'\'' {
                if self.b.get(self.i) == Some(&b'\'') {
                    raw.push(b'\'');
                    self.i += 1;
                } else {
                    break;
                }
            } else if c != b'\n' && c != b'\r' {
                raw.push(c);
            }
        }
        Ok(decode_string(&raw))
    }
}

/// Decodes the escapes of ISO 10303-21 strings: `\\`, `\S\c`, `\X\hh`,
/// `\X2\hhhh…\X0\` and `\X4\hhhhhhhh…\X0\`; `\P?\` code page switches are
/// dropped. Bytes that are not escapes are read as UTF-8 (lossily), which
/// many exporters write directly.
pub fn decode_string(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut plain: Vec<u8> = Vec::new();
    let flush = |plain: &mut Vec<u8>, out: &mut String| {
        out.push_str(&String::from_utf8_lossy(plain));
        plain.clear();
    };
    let hex = |s: &[u8]| std::str::from_utf8(s).ok().and_then(|s| u32::from_str_radix(s, 16).ok());
    let mut i = 0;
    while i < raw.len() {
        let rest = &raw[i..];
        if rest.starts_with(b"\\\\") {
            plain.push(b'\\');
            i += 2;
        } else if rest.starts_with(b"\\X2\\") || rest.starts_with(b"\\X4\\") {
            let width = if rest[2] == b'2' { 4 } else { 8 };
            let end = rest[4..].windows(4).position(|w| w == b"\\X0\\").map(|p| p + 4);
            let Some(end) = end else {
                plain.extend_from_slice(rest);
                break;
            };
            flush(&mut plain, &mut out);
            let units: Vec<u32> = rest[4..end].chunks(width).filter_map(hex).collect();
            if width == 4 {
                let u16s: Vec<u16> = units.iter().map(|&u| u as u16).collect();
                out.push_str(&String::from_utf16_lossy(&u16s));
            } else {
                out.extend(units.iter().filter_map(|&u| char::from_u32(u)));
            }
            i += end + 4;
        } else if rest.starts_with(b"\\X\\") && rest.len() >= 5 {
            flush(&mut plain, &mut out);
            if let Some(c) = hex(&rest[3..5]).and_then(char::from_u32) {
                out.push(c);
            }
            i += 5;
        } else if rest.starts_with(b"\\S\\") && rest.len() >= 4 {
            flush(&mut plain, &mut out);
            if let Some(c) = char::from_u32(u32::from(rest[3]) + 128) {
                out.push(c);
            }
            i += 4;
        } else if rest.starts_with(b"\\P") && rest.len() >= 4 && rest[3] == b'\\' {
            i += 4;
        } else {
            plain.push(raw[i]);
            i += 1;
        }
    }
    flush(&mut plain, &mut out);
    out
}

/// A partly built list: the typed name it belongs to, if any, and its
/// values so far.
struct Frame {
    typed: Option<String>,
    values: Vec<Value>,
}

/// Reads one parameter list after its opening `(`, up to and including
/// the matching `)`, without recursion.
fn read_list(lx: &mut Lexer) -> Result<Vec<Value>> {
    let mut stack = vec![Frame { typed: None, values: Vec::new() }];
    let mut pending: Option<String> = None;
    loop {
        let Some(t) = lx.next()? else { return Err(err("the file is truncated")) };
        if pending.is_some() && t != Tok::Open {
            return Err(err("a typed value without its parameters"));
        }
        let v = match t {
            Tok::Open => {
                if stack.len() >= MAX_DEPTH {
                    return Err(err("lists are nested too deeply"));
                }
                stack.push(Frame { typed: pending.take(), values: Vec::new() });
                continue;
            }
            Tok::Close => {
                let f = stack.pop().expect("a frame is open");
                let v = match f.typed {
                    Some(name) => Value::Typed(name, f.values),
                    None => Value::List(f.values),
                };
                match stack.last_mut() {
                    Some(parent) => {
                        parent.values.push(v);
                        continue;
                    }
                    None => {
                        let Value::List(values) = v else { unreachable!("the outer frame is untyped") };
                        return Ok(values);
                    }
                }
            }
            Tok::Comma => continue,
            Tok::Keyword(k) => {
                pending = Some(k);
                continue;
            }
            Tok::Dollar => Value::Null,
            Tok::Star => Value::Derived,
            Tok::Int(i) => Value::Int(i),
            Tok::Real(r) => Value::Real(r),
            Tok::Str(s) => Value::Str(s),
            Tok::Enum(s) => Value::Enum(s),
            Tok::Ref(r) => Value::Ref(r),
            Tok::Binary(s) => Value::Binary(s),
            Tok::Semi | Tok::Eq => return Err(err("a parameter list is not closed")),
        };
        stack.last_mut().expect("a frame is open").values.push(v);
    }
}

fn expect(lx: &mut Lexer, want: Tok) -> Result<()> {
    match lx.next()? {
        Some(t) if t == want => Ok(()),
        Some(_) => Err(err("unexpected token")),
        None => Err(err("the file is truncated")),
    }
}

/// Reads `NAME(args)` or, for a complex instance, `(A(..) B(..))`, then the
/// closing `;`.
fn read_record(lx: &mut Lexer) -> Result<Entity> {
    let e = match lx.next()? {
        Some(Tok::Keyword(name)) => {
            expect(lx, Tok::Open)?;
            Entity { name, args: read_list(lx)? }
        }
        Some(Tok::Open) => {
            let parts = read_list(lx)?;
            Entity { name: String::new(), args: parts }
        }
        Some(_) => return Err(err("unexpected token")),
        None => return Err(err("the file is truncated")),
    };
    expect(lx, Tok::Semi)?;
    Ok(e)
}

/// Parses a STEP physical file.
pub fn parse(bytes: &[u8]) -> Result<StepFile> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut lx = Lexer { b: bytes, i: 0 };
    if lx.next()? != Some(Tok::Keyword("ISO-10303-21".into())) {
        return Err(err("not a STEP file"));
    }
    expect(&mut lx, Tok::Semi)?;
    let mut file = StepFile::default();
    let mut seen_data = false;
    loop {
        match lx.next()? {
            Some(Tok::Keyword(k)) if k == "HEADER" => {
                expect(&mut lx, Tok::Semi)?;
                loop {
                    match lx.next()? {
                        Some(Tok::Keyword(k)) if k == "ENDSEC" => {
                            expect(&mut lx, Tok::Semi)?;
                            break;
                        }
                        Some(Tok::Keyword(k)) => {
                            expect(&mut lx, Tok::Open)?;
                            let args = read_list(&mut lx)?;
                            expect(&mut lx, Tok::Semi)?;
                            if k == "FILE_SCHEMA" {
                                let names = args.first().and_then(Value::as_list).unwrap_or_default();
                                file.schemas = names.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                            }
                        }
                        Some(_) => return Err(err("unexpected token in the header")),
                        None => return Err(err("the file is truncated")),
                    }
                }
            }
            Some(Tok::Keyword(k)) if k == "DATA" => {
                // `DATA('name', ...);` in later editions.
                match lx.next()? {
                    Some(Tok::Semi) => {}
                    Some(Tok::Open) => {
                        read_list(&mut lx)?;
                        expect(&mut lx, Tok::Semi)?;
                    }
                    _ => return Err(err("unexpected token")),
                }
                seen_data = true;
                loop {
                    match lx.next()? {
                        Some(Tok::Keyword(k)) if k == "ENDSEC" => {
                            expect(&mut lx, Tok::Semi)?;
                            break;
                        }
                        Some(Tok::Ref(id)) => {
                            expect(&mut lx, Tok::Eq)?;
                            let e = read_record(&mut lx)?;
                            file.entities.insert(id, e);
                        }
                        Some(_) => return Err(err("unexpected token in the data section")),
                        None => return Err(err("the file is truncated")),
                    }
                }
            }
            Some(Tok::Keyword(k)) if k == "END-ISO-10303-21" => break,
            Some(Tok::Keyword(_)) => {
                // Another section (ANCHOR, REFERENCE, SIGNATURE): skip its
                // records up to ENDSEC.
                loop {
                    match lx.next()? {
                        Some(Tok::Keyword(k)) if k == "ENDSEC" => break,
                        Some(_) => {}
                        None => return Err(err("the file is truncated")),
                    }
                }
            }
            Some(_) => return Err(err("unexpected token")),
            None => break,
        }
    }
    if !seen_data {
        return Err(err("the file has no DATA section"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_instances_with_every_kind_of_value() {
        let src = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('ViewDefinition'),'2;1');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n\
            /* a comment */\n#1= IFCCARTESIANPOINT((1.,-2.5E1,3));\n#2=IFCX($,*,.T.,'it''s',#1,\n  (#1,(1,2)),IFCLABEL('a'),\"0F\");\n\
            #3=(IFCA(1)IFCB(2));\nENDSEC;\nEND-ISO-10303-21;\n";
        let f = parse(src.as_bytes()).unwrap();
        assert_eq!(f.schemas, ["IFC4"]);
        let p = f.get(1).unwrap();
        assert_eq!(p.name, "IFCCARTESIANPOINT");
        assert_eq!(p.args[0], Value::List(vec![Value::Real(1.0), Value::Real(-25.0), Value::Int(3)]));
        let x = f.get(2).unwrap();
        assert_eq!(x.args[0], Value::Null);
        assert_eq!(x.args[1], Value::Derived);
        assert_eq!(x.args[2], Value::Enum("T".into()));
        assert_eq!(x.args[3].as_str(), Some("it's"));
        assert_eq!(x.args[4], Value::Ref(1));
        assert_eq!(x.args[5], Value::List(vec![Value::Ref(1), Value::List(vec![Value::Int(1), Value::Int(2)])]));
        assert_eq!(x.args[6].as_str(), Some("a"));
        assert_eq!(x.args[7], Value::Binary("0F".into()));
        assert_eq!(f.get(3).unwrap().name, "");
        assert_eq!(f.get(3).unwrap().args.len(), 2);
    }

    #[test]
    fn decodes_string_escapes() {
        assert_eq!(decode_string(br"Caf\X\E9"), "Caf\u{e9}");
        assert_eq!(decode_string(br"\X2\00FC00DF\X0\x"), "\u{fc}\u{df}x");
        assert_eq!(decode_string(br"a\\b"), "a\\b");
        assert_eq!(decode_string(br"\PA\\S\D"), "\u{c4}");
        assert_eq!(decode_string("Größe".as_bytes()), "Größe");
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        let head = "ISO-10303-21;HEADER;ENDSEC;DATA;";
        for bad in [
            "",
            "hello",
            "ISO-10303-21;",
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'",
            &format!("{head}#1=IFCX((1,2);"),
            &format!("{head}#1=IFCX('open);ENDSEC;"),
            &format!("{head}#1=IFCX(1);"),
            &format!("{head}#1=IFCX(1)"),
            &format!("{head}#1=IFCX(@);ENDSEC;"),
            &format!("{head}#1=IFCX(IFCLABEL);ENDSEC;"),
            &format!("{head}#=IFCX(1);ENDSEC;"),
            &format!("{head}/* never closed"),
        ] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
        // Every prefix of a good file fails cleanly or parses.
        let good = format!("{head}#1=IFCX((1,(2.,'a')),.E.,$);ENDSEC;END-ISO-10303-21;");
        for n in 0..good.len() {
            let _ = parse(&good.as_bytes()[..n]);
        }
        assert!(parse(good.as_bytes()).is_ok());
    }

    #[test]
    fn deep_nesting_is_refused_without_recursion() {
        let depth = 200_000;
        let src = format!("ISO-10303-21;HEADER;ENDSEC;DATA;#1=IFCX({}1{});ENDSEC;", "(".repeat(depth), ")".repeat(depth));
        let e = parse(src.as_bytes()).unwrap_err();
        assert!(e.0.contains("nested"), "{e}");
        // Up to the limit is fine.
        let ok = MAX_DEPTH - 1;
        let src = format!("ISO-10303-21;HEADER;ENDSEC;DATA;#1=IFCX({}1{});ENDSEC;", "(".repeat(ok - 1), ")".repeat(ok - 1));
        assert!(parse(src.as_bytes()).is_ok());
    }
}
