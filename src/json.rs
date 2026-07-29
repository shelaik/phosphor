//! Minimal, allocation-light JSON scanner over a byte slice.
//!
//! It is NOT a full DOM parser: it exposes primitives so callers can walk an
//! object, capture only the keys they care about, and cheaply *skip* everything
//! else (including multi-megabyte strings) without allocating. This is what lets
//! us stream 540 MB of transcripts fast.

/// Escape a string for embedding inside a JSON string literal.
pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

pub struct P<'a> {
    pub b: &'a [u8],
    pub i: usize,
}

impl<'a> P<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        P { b, i: 0 }
    }

    #[inline]
    fn peek(&self) -> u8 {
        if self.i < self.b.len() {
            self.b[self.i]
        } else {
            0
        }
    }

    /// Skip whitespace and return the next byte without consuming it.
    pub fn peek_ws(&mut self) -> u8 {
        self.ws();
        self.peek()
    }

    #[inline]
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.peek() == c {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn lit(&mut self, kw: &[u8]) -> Option<()> {
        if self.i + kw.len() <= self.b.len() && &self.b[self.i..self.i + kw.len()] == kw {
            self.i += kw.len();
            Some(())
        } else {
            None
        }
    }

    // ---- object/array iteration -------------------------------------------

    /// Consume `{`. Returns true if there is at least one entry (cursor at the
    /// first key). Returns false for `{}` (already fully consumed) or non-object.
    pub fn obj_begin(&mut self) -> bool {
        if !self.eat(b'{') {
            return false;
        }
        self.ws();
        if self.peek() == b'}' {
            self.i += 1;
            return false;
        }
        true
    }

    /// Read the current object key and consume the following `:`.
    pub fn obj_key(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != b'"' {
            return None;
        }
        let k = self.string()?;
        self.ws();
        if self.peek() != b':' {
            return None;
        }
        self.i += 1;
        self.ws();
        Some(k)
    }

    /// After a value: true if another entry follows (cursor at next key),
    /// false if the object ended.
    pub fn obj_sep(&mut self) -> bool {
        self.ws();
        match self.peek() {
            b',' => {
                self.i += 1;
                self.ws();
                true
            }
            b'}' => {
                self.i += 1;
                false
            }
            _ => false,
        }
    }

    pub fn arr_begin(&mut self) -> bool {
        if !self.eat(b'[') {
            return false;
        }
        self.ws();
        if self.peek() == b']' {
            self.i += 1;
            return false;
        }
        true
    }

    pub fn arr_sep(&mut self) -> bool {
        self.ws();
        match self.peek() {
            b',' => {
                self.i += 1;
                self.ws();
                true
            }
            b']' => {
                self.i += 1;
                false
            }
            _ => false,
        }
    }

    // ---- scalar capture ----------------------------------------------------

    /// Capture a string value, fully consuming it. If the value is not a string
    /// (null, number, etc.) it is skipped and None is returned. Either way the
    /// value is consumed, so the caller never desyncs.
    pub fn take_string(&mut self) -> Option<String> {
        self.ws();
        if self.peek() == b'"' {
            self.string()
        } else {
            let _ = self.skip();
            None
        }
    }

    /// Capture a numeric value as f64, fully consuming whatever is there.
    pub fn take_number(&mut self) -> f64 {
        self.ws();
        match self.peek() {
            b'-' | b'0'..=b'9' => self.number().unwrap_or(0.0),
            _ => {
                let _ = self.skip();
                0.0
            }
        }
    }

    /// Capture a boolean value, consuming whatever is there.
    pub fn take_bool(&mut self) -> bool {
        self.ws();
        let v = self.peek() == b't';
        let _ = self.skip();
        v
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.i;
        if self.peek() == b'-' {
            self.i += 1;
        }
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.peek() == b'.' {
            self.i += 1;
            while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if matches!(self.peek(), b'e' | b'E') {
            self.i += 1;
            if matches!(self.peek(), b'+' | b'-') {
                self.i += 1;
            }
            while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse::<f64>()
            .ok()
    }

    fn hex4(&mut self) -> Option<u32> {
        if self.i + 4 > self.b.len() {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.b[self.i];
            self.i += 1;
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a' + 10) as u32,
                b'A'..=b'F' => (c - b'A' + 10) as u32,
                _ => return None,
            };
            v = v * 16 + d;
        }
        Some(v)
    }

    /// Decode a JSON string into an owned UTF-8 String (cursor at opening quote).
    pub fn string(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != b'"' {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        while self.i < self.b.len() {
            let c = self.b[self.i];
            self.i += 1;
            match c {
                b'"' => return Some(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            let ch = if (0xD800..=0xDBFF).contains(&cp) {
                                // high surrogate, expect a following \uXXXX low surrogate
                                if self.b.get(self.i) == Some(&b'\\')
                                    && self.b.get(self.i + 1) == Some(&b'u')
                                {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    // Combine only when `lo` is a REAL low surrogate; else
                                    // `lo - 0xDC00` underflows (wraps in release → wrong char,
                                    // panics in debug). A stray value simply yields nothing.
                                    if (0xDC00..=0xDFFF).contains(&lo) {
                                        let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                        char::from_u32(c)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                char::from_u32(cp)
                            };
                            if let Some(ch) = ch {
                                let mut buf = [0u8; 4];
                                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                            }
                        }
                        _ => {}
                    }
                }
                _ => out.push(c),
            }
        }
        None
    }

    fn skip_string(&mut self) -> Option<()> {
        if self.peek() != b'"' {
            return None;
        }
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Some(());
                }
                _ => self.i += 1,
            }
        }
        None
    }

    /// Skip any JSON value without allocating for strings.
    pub fn skip(&mut self) -> Option<()> {
        self.skip_depth(0)
    }

    /// Recursion guard: refuse to descend past `MAX_DEPTH` nested containers, so a
    /// pathologically deep (or malicious) document can't blow the stack. Real
    /// transcripts nest only a handful of levels deep.
    fn skip_depth(&mut self, depth: u32) -> Option<()> {
        const MAX_DEPTH: u32 = 512;
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match self.peek() {
            b'"' => self.skip_string(),
            b'{' => {
                if self.obj_begin() {
                    loop {
                        if self.obj_key().is_none() {
                            break;
                        }
                        self.skip_depth(depth + 1)?;
                        if !self.obj_sep() {
                            break;
                        }
                    }
                }
                Some(())
            }
            b'[' => {
                if self.arr_begin() {
                    loop {
                        self.skip_depth(depth + 1)?;
                        if !self.arr_sep() {
                            break;
                        }
                    }
                }
                Some(())
            }
            b't' => self.lit(b"true"),
            b'f' => self.lit(b"false"),
            b'n' => self.lit(b"null"),
            0 => None,
            _ => self.number().map(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::P;

    #[test]
    fn unpaired_low_surrogate_does_not_underflow() {
        // \uD800 is a high surrogate but the following A ('A') is NOT a low
        // surrogate. Before the fix, `lo - 0xDC00` underflowed (panic in debug /
        // wrong char in release). Now the bad pair yields nothing and parsing
        // continues — reaching this assertion proves no panic/underflow.
        let mut p = P::new(b"\"x\\uD800\\u0041y\"");
        let s = p.string().expect("must parse without panicking");
        assert!(s.starts_with('x') && s.ends_with('y'));
    }

    #[test]
    fn valid_surrogate_pair_still_decodes() {
        // 😀 = U+1F600 😀 — the happy path is unaffected by the guard.
        let mut p = P::new("\"\\uD83D\\uDE00\"".as_bytes());
        assert_eq!(p.string().unwrap(), "😀");
    }
}
