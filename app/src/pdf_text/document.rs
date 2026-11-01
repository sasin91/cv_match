//! A PDF file: its cross reference, objects, stream filters and pages.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::syntax::{Dict, Lexer, Object, Stream};
use super::zlib;

#[derive(Clone, Copy, Debug)]
enum Entry {
    /// Byte offset of `n g obj`.
    At(usize),
    /// Index within an object stream.
    InStream(u32, u32),
}

pub struct Document {
    data: Vec<u8>,
    xref: HashMap<u32, Entry>,
    trailer: Dict,
    objects: RefCell<HashMap<u32, Object>>,
    /// The objects being read now: one met again while it is read (a stream whose /Length is
    /// itself, an object stream inside itself) reads as null instead of recursing forever.
    loading: RefCell<HashSet<u32>>,
    pub pages: Vec<PageRef>,
}

/// A page from the page tree, with what it inherits filled in.
#[derive(Clone, Debug)]
pub struct PageRef {
    pub dict: Dict,
    pub resources: Dict,
    /// The visible box, `[x0, y0, x1, y1]` in points: the crop box where
    /// there is one.
    pub bounds: [f64; 4],
    pub rotate: i64,
}

/// Stream data after its filters, and the image filter left for an image
/// decoder, if one ends the chain.
pub struct Decoded {
    pub data: Vec<u8>,
    pub image_filter: Option<String>,
}

impl Document {
    pub fn open(data: Vec<u8>) -> Result<Document, String> {
        if !data.starts_with(b"%PDF") && !data[..data.len().min(1024)].windows(4).any(|w| w == b"%PDF") {
            return Err("Not a PDF file".into());
        }
        let mut doc = Document {
            data,
            xref: HashMap::new(),
            trailer: Dict::new(),
            objects: RefCell::new(HashMap::new()),
            loading: RefCell::new(HashSet::new()),
            pages: vec![],
        };
        if doc.read_xref().is_err() || !doc.trailer.contains_key("Root") {
            doc.rebuild_xref();
        }
        if doc.trailer.contains_key("Encrypt") {
            return Err("Encrypted PDFs are not supported yet".into());
        }
        let root = doc.trailer.get("Root").cloned().ok_or("The PDF has no catalog")?;
        let catalog = doc.resolve(&root);
        let pages = catalog
            .as_dict()
            .and_then(|c| c.get("Pages"))
            .cloned()
            .ok_or("The PDF has no pages")?;
        let mut out = vec![];
        doc.walk_pages(&pages, &Inherited::default(), &mut out, 0);
        if out.is_empty() {
            return Err("The PDF has no pages".into());
        }
        doc.pages = out;
        Ok(doc)
    }

    fn startxref(&self) -> Option<usize> {
        let tail = self.data.len().saturating_sub(2048);
        let at = self.data[tail..].windows(9).rposition(|w| w == b"startxref")? + tail + 9;
        Lexer::new(&self.data, at).object(false)?.as_i64().map(|n| n as usize)
    }

    fn read_xref(&mut self) -> Result<(), ()> {
        let mut next = Some(self.startxref().ok_or(())?);
        let mut seen = vec![];
        while let Some(offset) = next.take() {
            if seen.contains(&offset) || offset >= self.data.len() {
                break;
            }
            seen.push(offset);
            let trailer = if self.data[offset..].starts_with(b"xref") {
                self.read_table(offset + 4)?
            } else {
                self.read_xref_stream(offset)?
            };
            // Hybrid files keep a cross reference stream beside the table.
            if let Some(at) = trailer.get("XRefStm").and_then(Object::as_i64) {
                let _ = self.read_xref_stream(at as usize);
            }
            next = trailer.get("Prev").and_then(Object::as_i64).map(|p| p as usize);
            // The newest trailer wins; older ones fill in what it lacks.
            for (k, v) in trailer {
                self.trailer.entry(k).or_insert(v);
            }
        }
        Ok(())
    }

    fn read_table(&mut self, at: usize) -> Result<Dict, ()> {
        let mut lex = Lexer::new(&self.data, at);
        loop {
            match lex.object(false).ok_or(())? {
                Object::Keyword(k) if k == "trailer" => break,
                Object::Int(start) => {
                    let count = lex.object(false).and_then(|o| o.as_i64()).ok_or(())?;
                    for i in 0..count {
                        let offset = lex.object(false).and_then(|o| o.as_i64()).ok_or(())?;
                        let _generation = lex.object(false);
                        let kind = lex.object(false).ok_or(())?;
                        let id = (start + i) as u32;
                        if kind.is_keyword("n") && offset > 0 {
                            self.xref.entry(id).or_insert(Entry::At(offset as usize));
                        }
                    }
                }
                _ => return Err(()),
            }
        }
        match lex.object(true) {
            Some(Object::Dict(d)) => Ok(d),
            _ => Err(()),
        }
    }

    fn read_xref_stream(&mut self, at: usize) -> Result<Dict, ()> {
        let Some(Object::Stream(stream)) = self.parse_at(at).map(|(_, o)| o) else {
            return Err(());
        };
        let data = self.decode(&stream).ok_or(())?.data;
        let dict = &stream.dict;
        let widths: Vec<usize> = dict
            .get("W")
            .and_then(Object::as_array)
            .ok_or(())?
            .iter()
            .map(|w| w.as_i64().unwrap_or(0) as usize)
            .collect();
        if widths.len() < 3 {
            return Err(());
        }
        let size = dict.get("Size").and_then(Object::as_i64).unwrap_or(0);
        let index: Vec<i64> = match dict.get("Index").and_then(Object::as_array) {
            Some(index) => index.iter().filter_map(Object::as_i64).collect(),
            None => vec![0, size],
        };
        let row = widths.iter().sum::<usize>();
        let field = |bytes: &[u8]| bytes.iter().fold(0u64, |n, &b| n << 8 | b as u64);
        let mut rows = data.chunks_exact(row.max(1));
        for pair in index.as_chunks::<2>().0 {
            for id in pair[0]..pair[0] + pair[1] {
                let Some(r) = rows.next() else { break };
                let (a, rest) = r.split_at(widths[0]);
                let (b, c) = rest.split_at(widths[1]);
                // An absent type field means type 1.
                let kind = if widths[0] == 0 { 1 } else { field(a) };
                let entry = match kind {
                    1 => Entry::At(field(b) as usize),
                    2 => Entry::InStream(field(b) as u32, field(c) as u32),
                    _ => continue,
                };
                self.xref.entry(id as u32).or_insert(entry);
            }
        }
        Ok(dict.clone())
    }

    /// For files whose cross reference is missing or wrong: finds every
    /// `n g obj` in the file, and the trailer or catalog.
    fn rebuild_xref(&mut self) {
        self.xref.clear();
        let data = &self.data;
        let mut i = 0;
        while let Some(found) = data[i..].windows(3).position(|w| w == b"obj") {
            let at = i + found;
            i = at + 3;
            // Back over `n g ` to the object number.
            let mut start = at;
            let mut fields = 0;
            while start > 0 && fields < 2 {
                while start > 0 && data[start - 1] == b' ' {
                    start -= 1;
                }
                let end = start;
                while start > 0 && data[start - 1].is_ascii_digit() {
                    start -= 1;
                }
                if start == end {
                    break;
                }
                fields += 1;
            }
            if fields == 2
                && let Some(id) = Lexer::new(data, start).object(false).and_then(|o| o.as_i64())
            {
                self.xref.insert(id as u32, Entry::At(start));
            }
        }
        if let Some(at) = data.windows(7).rposition(|w| w == b"trailer")
            && let Some(Object::Dict(d)) = Lexer::new(data, at + 7).object(true)
        {
            self.trailer = d;
        }
        if !self.trailer.contains_key("Root") {
            let ids: Vec<u32> = self.xref.keys().copied().collect();
            for id in ids {
                if self
                    .get(id)
                    .as_dict()
                    .and_then(|d| d.get("Type"))
                    .and_then(Object::as_name)
                    == Some("Catalog")
                {
                    self.trailer.insert("Root".into(), Object::Ref(id, 0));
                    break;
                }
            }
        }
    }

    /// Reads `n g obj ... endobj` at `at`.
    fn parse_at(&self, at: usize) -> Option<(u32, Object)> {
        let mut lex = Lexer::new(&self.data, at);
        let id = lex.object(false)?.as_i64()? as u32;
        lex.object(false)?;
        if !lex.object(false)?.is_keyword("obj") {
            return None;
        }
        let object = lex.object(true)?;
        let Object::Dict(dict) = object else {
            return Some((id, object));
        };
        if !lex.object(false).is_some_and(|k| k.is_keyword("stream")) {
            return Some((id, Object::Dict(dict)));
        }
        // The data starts after the end of line that follows `stream`.
        let mut start = lex.pos;
        if self.data.get(start) == Some(&b'\r') {
            start += 1;
        }
        if self.data.get(start) == Some(&b'\n') {
            start += 1;
        }
        let length = dict
            .get("Length")
            .map(|l| self.resolve(l))
            .and_then(|l| l.as_i64())
            .map(|l| l as usize);
        let end = match length {
            Some(n) if self.data.get(start + n..).is_some_and(ends_stream) => start + n,
            // A wrong length: look for `endstream` instead.
            _ => {
                let found = self.data[start..].windows(9).position(|w| w == b"endstream")? + start;
                let mut end = found;
                while end > start && matches!(self.data[end - 1], b'\r' | b'\n') {
                    end -= 1;
                }
                end
            }
        };
        Some((
            id,
            Object::Stream(Rc::new(Stream {
                dict,
                data: self.data[start..end].to_vec(),
            })),
        ))
    }

    pub fn get(&self, id: u32) -> Object {
        if let Some(o) = self.objects.borrow().get(&id) {
            return o.clone();
        }
        if !self.loading.borrow_mut().insert(id) {
            return Object::Null;
        }
        let object = match self.xref.get(&id) {
            Some(Entry::At(at)) => self.parse_at(*at).filter(|(n, _)| *n == id).map(|(_, o)| o),
            Some(&Entry::InStream(stream, index)) => self.in_object_stream(stream, index),
            None => None,
        }
        .unwrap_or(Object::Null);
        self.loading.borrow_mut().remove(&id);
        self.objects.borrow_mut().insert(id, object.clone());
        object
    }

    fn in_object_stream(&self, stream: u32, index: u32) -> Option<Object> {
        let Object::Stream(s) = self.get(stream) else {
            return None;
        };
        let data = self.decode(&s)?.data;
        let first = s.dict.get("First")?.as_i64()? as usize;
        let mut header = Lexer::new(&data, 0);
        let mut offset = None;
        for i in 0..=index {
            let _id = header.object(false)?;
            let at = header.object(false)?.as_i64()? as usize;
            if i == index {
                offset = Some(at);
            }
        }
        Lexer::new(&data, first + offset?).object(true)
    }

    /// Follows references.
    pub fn resolve(&self, object: &Object) -> Object {
        let mut object = object.clone();
        for _ in 0..32 {
            match object {
                Object::Ref(id, _) => object = self.get(id),
                _ => return object,
            }
        }
        Object::Null
    }

    /// `dict[key]`, resolved.
    pub fn lookup(&self, dict: &Dict, key: &str) -> Object {
        dict.get(key).map(|o| self.resolve(o)).unwrap_or(Object::Null)
    }

    pub fn decode(&self, stream: &Stream) -> Option<Decoded> {
        let filters = match self.lookup(&stream.dict, "Filter") {
            Object::Name(n) => vec![n],
            Object::Array(a) => a
                .iter()
                .filter_map(|f| self.resolve(f).as_name().map(String::from))
                .collect(),
            _ => vec![],
        };
        let parms = match self.lookup(&stream.dict, "DecodeParms") {
            Object::Array(a) => a.iter().map(|p| self.resolve(p)).collect(),
            Object::Null => vec![],
            p => vec![p],
        };
        let mut data = stream.data.clone();
        for (i, filter) in filters.iter().enumerate() {
            let parms = parms.get(i).and_then(Object::as_dict);
            data = match filter.as_str() {
                "FlateDecode" | "Fl" => predict(zlib::inflate_zlib(&data)?, parms),
                "LZWDecode" | "LZW" => predict(lzw(&data, parms), parms),
                "ASCIIHexDecode" | "AHx" => ascii_hex(&data),
                "ASCII85Decode" | "A85" => ascii85(&data),
                "RunLengthDecode" | "RL" => run_length(&data),
                // Image codecs: the caller hands these to an image decoder.
                "DCTDecode" | "DCT" | "JPXDecode" | "JBIG2Decode" | "CCITTFaxDecode" | "CCF" => {
                    return Some(Decoded {
                        data,
                        image_filter: Some(filter.clone()),
                    });
                }
                _ => return None,
            };
        }
        Some(Decoded {
            data,
            image_filter: None,
        })
    }

    /// A stream's data after its filters, for content and fonts.
    pub fn stream_data(&self, object: &Object) -> Option<Vec<u8>> {
        match self.resolve(object) {
            Object::Stream(s) => self.decode(&s).filter(|d| d.image_filter.is_none()).map(|d| d.data),
            _ => None,
        }
    }

    fn walk_pages(&self, node: &Object, inherited: &Inherited, out: &mut Vec<PageRef>, depth: usize) {
        if depth > 64 {
            return;
        }
        let Object::Dict(dict) = self.resolve(node) else {
            return;
        };
        let mut here = inherited.clone();
        if let Object::Dict(r) = self.lookup(&dict, "Resources") {
            here.resources = Some(r);
        }
        if let Some(b) = self.rect(&dict, "MediaBox") {
            here.media = Some(b);
        }
        if let Some(b) = self.rect(&dict, "CropBox") {
            here.crop = Some(b);
        }
        if let Some(r) = self.lookup(&dict, "Rotate").as_i64() {
            here.rotate = r.rem_euclid(360);
        }
        match self.lookup(&dict, "Kids") {
            Object::Array(kids) => {
                for kid in &kids {
                    self.walk_pages(kid, &here, out, depth + 1);
                }
            }
            _ => {
                let media = here.media.unwrap_or([0.0, 0.0, 612.0, 792.0]);
                // The crop box, clipped to the media box.
                let bounds = match here.crop {
                    Some(c) => [
                        c[0].max(media[0]),
                        c[1].max(media[1]),
                        c[2].min(media[2]),
                        c[3].min(media[3]),
                    ],
                    None => media,
                };
                let bounds = if bounds[2] - bounds[0] > 1.0 && bounds[3] - bounds[1] > 1.0 {
                    bounds
                } else {
                    media
                };
                out.push(PageRef {
                    resources: here.resources.unwrap_or_default(),
                    bounds,
                    rotate: here.rotate,
                    dict,
                });
            }
        }
    }

    pub fn rect(&self, dict: &Dict, key: &str) -> Option<[f64; 4]> {
        let Object::Array(a) = self.lookup(dict, key) else {
            return None;
        };
        let v: Vec<f64> = a.iter().filter_map(|o| self.resolve(o).as_f64()).collect();
        (v.len() == 4).then(|| [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
    }
}

#[derive(Clone, Default)]
struct Inherited {
    resources: Option<Dict>,
    media: Option<[f64; 4]>,
    crop: Option<[f64; 4]>,
    rotate: i64,
}

fn ends_stream(rest: &[u8]) -> bool {
    let trimmed = rest
        .iter()
        .position(|b| !super::syntax::is_white(*b))
        .unwrap_or(rest.len());
    rest[trimmed..].starts_with(b"endstream")
}

/// Undoes a PNG or TIFF predictor.
fn predict(data: Vec<u8>, parms: Option<&Dict>) -> Vec<u8> {
    let Some(parms) = parms else { return data };
    let int = |k: &str, default: i64| parms.get(k).and_then(Object::as_i64).unwrap_or(default);
    let predictor = int("Predictor", 1);
    if predictor < 2 {
        return data;
    }
    let (colors, bits, columns) = (
        int("Colors", 1).max(1) as usize,
        int("BitsPerComponent", 8).max(1) as usize,
        int("Columns", 1).max(1) as usize,
    );
    let bpp = (colors * bits).div_ceil(8);
    let row = (colors * bits * columns).div_ceil(8);
    if predictor == 2 {
        // TIFF: each byte is a difference from the pixel to its left
        // (8 bits per component only).
        let mut out = data;
        if bits == 8 {
            for line in out.chunks_mut(row) {
                for i in bpp..line.len() {
                    line[i] = line[i].wrapping_add(line[i - bpp]);
                }
            }
        }
        return out;
    }
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row];
    for line in data.chunks(row + 1) {
        let (kind, line) = (line[0], &line[1..]);
        let mut current = vec![0u8; row];
        for i in 0..line.len() {
            let left = if i >= bpp { current[i - bpp] } else { 0 };
            let up = previous[i];
            let up_left = if i >= bpp { previous[i - bpp] } else { 0 };
            current[i] = line[i].wrapping_add(match kind {
                1 => left,
                2 => up,
                3 => ((left as u16 + up as u16) / 2) as u8,
                4 => paeth(left, up, up_left),
                _ => 0,
            });
        }
        out.extend_from_slice(&current[..line.len()]);
        previous = current;
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = data
        .iter()
        .take_while(|&&b| b != b'>')
        .filter_map(|&b| (b as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .map(|p| p[0] << 4 | p.get(1).copied().unwrap_or(0))
        .collect()
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut group = [0u32; 5];
    let mut n = 0;
    for &b in data {
        match b {
            b'~' => break,
            b'z' if n == 0 => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group[n] = (b - b'!') as u32;
                n += 1;
                if n == 5 {
                    let v = group.iter().fold(0u32, |v, &d| v.wrapping_mul(85).wrapping_add(d));
                    out.extend_from_slice(&v.to_be_bytes());
                    n = 0;
                }
            }
            _ => {}
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u32, |v, &d| v.wrapping_mul(85).wrapping_add(d));
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
    }
    out
}

fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let n = data[i] as usize;
        i += 1;
        match n {
            128 => break,
            0..=127 => {
                out.extend_from_slice(&data[i..(i + n + 1).min(data.len())]);
                i += n + 1;
            }
            _ => {
                if let Some(&b) = data.get(i) {
                    out.extend(std::iter::repeat_n(b, 257 - n));
                }
                i += 1;
            }
        }
    }
    out
}

fn lzw(data: &[u8], parms: Option<&Dict>) -> Vec<u8> {
    let early = parms
        .and_then(|p| p.get("EarlyChange"))
        .and_then(Object::as_i64)
        .unwrap_or(1) as usize;
    let mut table: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).collect();
    table.push(vec![]);
    table.push(vec![]);
    let (mut out, mut previous): (Vec<u8>, Option<Vec<u8>>) = (Vec::new(), None);
    let (mut bits, mut acc, mut width) = (0u32, 0u32, 9u32);
    for &b in data {
        acc = acc << 8 | b as u32;
        bits += 8;
        while bits >= width {
            let code = ((acc >> (bits - width)) & ((1 << width) - 1)) as usize;
            bits -= width;
            match code {
                256 => {
                    table.truncate(258);
                    width = 9;
                    previous = None;
                }
                257 => return out,
                _ => {
                    let entry = if code < table.len() {
                        table[code].clone()
                    } else if let Some(p) = &previous {
                        let mut e = p.clone();
                        e.push(p[0]);
                        e
                    } else {
                        return out;
                    };
                    out.extend_from_slice(&entry);
                    if let Some(mut p) = previous.take() {
                        p.push(entry[0]);
                        table.push(p);
                    }
                    previous = Some(entry);
                    let next = table.len() + early;
                    width = if next >= 2048 {
                        12
                    } else if next >= 1024 {
                        11
                    } else if next >= 512 {
                        10
                    } else {
                        9
                    };
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_ascii_filters() {
        assert_eq!(ascii_hex(b"48 65 6c 6c 6f>"), b"Hello");
        assert_eq!(ascii85(b"87cURD]i,\"Ebo80~>"), b"Hello World!");
        assert_eq!(run_length(&[2, b'a', b'b', b'c', 254, b'x', 128]), b"abcxxx");
    }

    #[test]
    fn undoes_png_predictors() {
        // Two rows of three bytes, `Sub` then `Up`.
        let data = vec![1, 1, 1, 1, 2, 1, 1, 1];
        let parms: Dict = [
            ("Predictor".to_string(), Object::Int(12)),
            ("Columns".into(), Object::Int(3)),
        ]
        .into();
        assert_eq!(predict(data, Some(&parms)), vec![1, 2, 3, 2, 3, 4]);
    }
}
