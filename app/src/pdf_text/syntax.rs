//! PDF objects and the lexer that reads them, shared by the file's
//! objects, page content streams and ToUnicode maps.

use std::collections::HashMap;
use std::rc::Rc;

pub type Dict = HashMap<String, Object>;

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Name(String),
    String(Vec<u8>),
    Array(Vec<Object>),
    Dict(Dict),
    Stream(Rc<Stream>),
    Ref(u32, u16),
    /// A bare word: a content stream operator, or `obj`, `stream` and the
    /// like in the file.
    Keyword(String),
}

#[derive(Debug, PartialEq)]
pub struct Stream {
    pub dict: Dict,
    /// Still encoded, as in the file.
    pub data: Vec<u8>,
}

impl Object {
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Object::Int(i) => Some(i as f64),
            Object::Real(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Object::Int(i) => Some(i),
            Object::Real(r) => Some(r as i64),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&str> {
        match self {
            Object::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Object::Array(a) => Some(a),
            _ => None,
        }
    }

    /// A dictionary, or a stream's dictionary.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Object::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_keyword(&self, word: &str) -> bool {
        matches!(self, Object::Keyword(k) if k == word)
    }
}

pub fn is_white(b: u8) -> bool {
    matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

pub fn is_delimiter(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// How deep arrays and dictionaries may nest in one object. Real PDFs nest a few levels; a
/// file nesting thousands would overflow the stack, so it reads as broken instead.
const MAX_NESTING: usize = 64;

pub struct Lexer<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    /// The arrays and dictionaries the object being read is inside.
    depth: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos, depth: 0 }
    }

    /// An array's or dictionary's items, read one level deeper; None past MAX_NESTING.
    fn nested<T>(&mut self, read: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        if self.depth >= MAX_NESTING {
            return None;
        }
        self.depth += 1;
        let read = read(self);
        self.depth -= 1;
        read
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    pub fn skip_white(&mut self) {
        while let Some(b) = self.peek() {
            if is_white(b) {
                self.pos += 1;
            } else if b == b'%' {
                while let Some(b) = self.peek() {
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    pub(crate) fn word(&mut self) -> &'a [u8] {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_white(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        &self.data[start..self.pos]
    }

    /// In a content stream, the next operator as a slice of the stream,
    /// without allocating, or `None` when an operand comes next. Operators
    /// are bare words, and `'` and `"`.
    pub fn operator(&mut self) -> Option<&'a str> {
        self.skip_white();
        let b = self.peek()?;
        if b == b'\'' || b == b'"' {
            self.pos += 1;
            return Some(if b == b'\'' { "'" } else { "\"" });
        }
        if !b.is_ascii_alphabetic() {
            return None;
        }
        let start = self.pos;
        let word = self.word();
        match word {
            // Operands spelled as words.
            b"true" | b"false" | b"null" => {
                self.pos = start;
                None
            }
            _ => std::str::from_utf8(word).ok(),
        }
    }

    /// The next object, or `None` at the end. With `refs`, `1 0 R` reads
    /// as a reference; content streams have none.
    pub fn object(&mut self, refs: bool) -> Option<Object> {
        self.skip_white();
        let b = self.peek()?;
        match b {
            b'/' => {
                self.pos += 1;
                let raw = self.word();
                let mut name = Vec::with_capacity(raw.len());
                let mut i = 0;
                while i < raw.len() {
                    match (
                        raw[i],
                        raw.get(i + 1).and_then(|&b| hex(b)),
                        raw.get(i + 2).and_then(|&b| hex(b)),
                    ) {
                        (b'#', Some(h), Some(l)) => {
                            name.push(h << 4 | l);
                            i += 3;
                        }
                        (b, _, _) => {
                            name.push(b);
                            i += 1;
                        }
                    }
                }
                Some(Object::Name(String::from_utf8_lossy(&name).into_owned()))
            }
            b'(' => Some(Object::String(self.literal())),
            b'<' if self.data.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                self.nested(|lex| {
                    let mut dict = Dict::new();
                    loop {
                        lex.skip_white();
                        if lex.data[lex.pos..].starts_with(b">>") {
                            lex.pos += 2;
                            break;
                        }
                        match lex.object(refs)? {
                            Object::Name(key) => {
                                let value = lex.object(refs)?;
                                if value.is_keyword(">>") {
                                    break;
                                }
                                dict.insert(key, value);
                            }
                            // Garbage between entries: skip it.
                            _ => continue,
                        }
                    }
                    Some(Object::Dict(dict))
                })
            }
            b'<' => {
                self.pos += 1;
                let mut bytes = Vec::new();
                let mut high = None;
                while let Some(b) = self.peek() {
                    self.pos += 1;
                    if b == b'>' {
                        break;
                    }
                    if let Some(v) = hex(b) {
                        match high.take() {
                            Some(h) => bytes.push(h << 4 | v),
                            None => high = Some(v),
                        }
                    }
                }
                // An odd digit out is followed by a zero.
                if let Some(h) = high {
                    bytes.push(h << 4);
                }
                Some(Object::String(bytes))
            }
            b'[' => {
                self.pos += 1;
                self.nested(|lex| {
                    let mut items = Vec::new();
                    loop {
                        lex.skip_white();
                        match lex.peek() {
                            None => break,
                            Some(b']') => {
                                lex.pos += 1;
                                break;
                            }
                            _ => items.push(lex.object(refs)?),
                        }
                    }
                    Some(Object::Array(items))
                })
            }
            b'>' if self.data.get(self.pos + 1) == Some(&b'>') => {
                self.pos += 2;
                Some(Object::Keyword(">>".into()))
            }
            b')' | b'>' | b']' | b'{' | b'}' => {
                self.pos += 1;
                Some(Object::Keyword((b as char).to_string()))
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let word = self.word();
                let number = number(word);
                if let (true, Object::Int(n)) = (refs, &number) {
                    // `n g R`: look ahead without consuming on a miss.
                    let back = self.pos;
                    let generation = {
                        self.skip_white();
                        let w = self.word();
                        (!w.is_empty() && w.iter().all(u8::is_ascii_digit)).then(|| number_u64(w))
                    };
                    if let Some(g) = generation {
                        self.skip_white();
                        if self.peek() == Some(b'R')
                            && self
                                .data
                                .get(self.pos + 1)
                                .is_none_or(|&b| is_white(b) || is_delimiter(b))
                        {
                            self.pos += 1;
                            return Some(Object::Ref(*n as u32, g as u16));
                        }
                    }
                    self.pos = back;
                }
                Some(number)
            }
            _ => {
                let word = self.word();
                if word.is_empty() {
                    // A stray delimiter; step over it.
                    self.pos += 1;
                    return Some(Object::Keyword(String::new()));
                }
                Some(match word {
                    b"true" => Object::Bool(true),
                    b"false" => Object::Bool(false),
                    b"null" => Object::Null,
                    _ => Object::Keyword(String::from_utf8_lossy(word).into_owned()),
                })
            }
        }
    }

    fn literal(&mut self) -> Vec<u8> {
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 0;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' if depth == 0 => break,
                b')' => {
                    depth -= 1;
                    out.push(b);
                }
                b'\\' => {
                    let Some(e) = self.peek() else { break };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'0'..=b'7' => {
                            let mut v = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        // A backslash at the end of a line continues it.
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        other => out.push(other),
                    }
                }
                _ => out.push(b),
            }
        }
        out
    }
}

fn number_u64(word: &[u8]) -> u64 {
    word.iter()
        .fold(0u64, |n, d| n.wrapping_mul(10).wrapping_add((d - b'0') as u64))
}

/// A number as content streams write them, tens of thousands to a chart:
/// integers and plain decimals directly, without allocating.
fn number(word: &[u8]) -> Object {
    let (negative, digits) = match word.first() {
        Some(b'-') => (true, &word[1..]),
        Some(b'+') => (false, &word[1..]),
        _ => (false, word),
    };
    let (mut whole, mut fraction, mut scale, mut point, mut any) = (0i64, 0i64, 1i64, false, false);
    for &d in digits {
        match d {
            b'0'..=b'9' if !point => {
                whole = whole.saturating_mul(10).saturating_add((d - b'0') as i64);
                any = true;
            }
            // Digits past what an f64 holds change nothing.
            b'0'..=b'9' if scale < 100_000_000_000_000_000 => {
                fraction = fraction * 10 + (d - b'0') as i64;
                scale *= 10;
                any = true;
            }
            b'0'..=b'9' => any = true,
            b'.' if !point => point = true,
            // Writers produce `--5` and other oddities: the slow way.
            _ => return odd_number(word),
        }
    }
    if !any {
        return odd_number(word);
    }
    let sign = if negative { -1 } else { 1 };
    if point {
        Object::Real(sign as f64 * (whole as f64 + fraction as f64 / scale as f64))
    } else {
        Object::Int(sign * whole)
    }
}

/// Keeps what parses of a malformed number: `--5`, `+-.5`.
fn odd_number(word: &[u8]) -> Object {
    let text = std::str::from_utf8(word).unwrap_or("0");
    let cleaned: String = text.trim_start_matches('+').replacen("--", "-", 1);
    Object::Real(cleaned.parse().unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> Object {
        Lexer::new(s.as_bytes(), 0).object(true).unwrap()
    }

    #[test]
    fn reads_objects() {
        assert_eq!(read("  % comment\n 42"), Object::Int(42));
        assert_eq!(read("-.5"), Object::Real(-0.5));
        assert_eq!(read("5."), Object::Real(5.0));
        assert_eq!(read("+12"), Object::Int(12));
        assert_eq!(read("-0.25"), Object::Real(-0.25));
        assert_eq!(read("--3"), Object::Real(-3.0));
        assert_eq!(read("3.14159265358979323846"), Object::Real(std::f64::consts::PI));
        assert_eq!(read("/A#20B"), Object::Name("A B".into()));
        assert_eq!(read("(a\\(b\\)\\101 (c))"), Object::String(b"a(b)A (c)".to_vec()));
        assert_eq!(read("<48 65 6>"), Object::String(b"He`".to_vec()));
        assert_eq!(
            read("[1 0 R 2 /X]"),
            Object::Array(vec![Object::Ref(1, 0), Object::Int(2), Object::Name("X".into())])
        );
        let Object::Dict(d) = read("<< /Type /Page /Kids [3 0 R] /N 5 >>") else {
            panic!()
        };
        assert_eq!(d["Type"], Object::Name("Page".into()));
        assert_eq!(d["N"], Object::Int(5));
    }
}
