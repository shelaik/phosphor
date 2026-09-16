//! A minimal PNG encoder and raster canvas, in `std` alone.
//!
//! `phosphor wrapped` produced only SVG, and X, Reddit and most chat clients
//! refuse to render one inline — the card existed but could not travel, which
//! was the whole point of making it. Pulling in an image crate to fix that would
//! cost more than the feature is worth for a binary that prides itself on having
//! no dependencies, so the pieces are here instead: a bitmap font, a canvas that
//! knows rectangles and text, and a real (compressing) PNG writer.
//!
//! The output is deliberately pixel-shaped. Phosphor already renders a pixel
//! logo and has a pixel mode, so 5×7 glyphs scaled up read as the house style
//! rather than as a downgrade from the vector version.
//!
//! **Compression matters here.** Stored (uncompressed) DEFLATE blocks are four
//! lines of code, but a 1200×630 card would weigh 2.2 MB — awkward to post, and
//! embarrassing for an image that is 95% flat colour. So the rows are `Up`
//! filtered (identical scanlines collapse to zeroes) and then run-length matched
//! into fixed-Huffman blocks, which brings the same card to a few tens of KB.

/// 5 columns per glyph, 7 rows per column (bit 0 = top). Covers printable
/// ASCII; anything else falls back to a hollow box so a missing glyph is
/// visible rather than silently swallowed.
const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;

#[rustfmt::skip]
const FONT: [(char, [u8; 5]); 96] = [
    (' ', [0x00,0x00,0x00,0x00,0x00]), ('!', [0x00,0x00,0x5F,0x00,0x00]),
    ('"', [0x00,0x07,0x00,0x07,0x00]), ('#', [0x14,0x7F,0x14,0x7F,0x14]),
    ('$', [0x24,0x2A,0x7F,0x2A,0x12]), ('%', [0x23,0x13,0x08,0x64,0x62]),
    ('&', [0x36,0x49,0x55,0x22,0x50]), ('\'',[0x00,0x05,0x03,0x00,0x00]),
    ('(', [0x00,0x1C,0x22,0x41,0x00]), (')', [0x00,0x41,0x22,0x1C,0x00]),
    ('*', [0x14,0x08,0x3E,0x08,0x14]), ('+', [0x08,0x08,0x3E,0x08,0x08]),
    (',', [0x00,0x50,0x30,0x00,0x00]), ('-', [0x08,0x08,0x08,0x08,0x08]),
    ('.', [0x00,0x60,0x60,0x00,0x00]), ('/', [0x20,0x10,0x08,0x04,0x02]),
    ('0', [0x3E,0x51,0x49,0x45,0x3E]), ('1', [0x00,0x42,0x7F,0x40,0x00]),
    ('2', [0x42,0x61,0x51,0x49,0x46]), ('3', [0x21,0x41,0x45,0x4B,0x31]),
    ('4', [0x18,0x14,0x12,0x7F,0x10]), ('5', [0x27,0x45,0x45,0x45,0x39]),
    ('6', [0x3C,0x4A,0x49,0x49,0x30]), ('7', [0x01,0x71,0x09,0x05,0x03]),
    ('8', [0x36,0x49,0x49,0x49,0x36]), ('9', [0x06,0x49,0x49,0x29,0x1E]),
    (':', [0x00,0x36,0x36,0x00,0x00]), (';', [0x00,0x56,0x36,0x00,0x00]),
    ('<', [0x08,0x14,0x22,0x41,0x00]), ('=', [0x14,0x14,0x14,0x14,0x14]),
    ('>', [0x00,0x41,0x22,0x14,0x08]), ('?', [0x02,0x01,0x51,0x09,0x06]),
    ('@', [0x32,0x49,0x79,0x41,0x3E]), ('A', [0x7E,0x11,0x11,0x11,0x7E]),
    ('B', [0x7F,0x49,0x49,0x49,0x36]), ('C', [0x3E,0x41,0x41,0x41,0x22]),
    ('D', [0x7F,0x41,0x41,0x22,0x1C]), ('E', [0x7F,0x49,0x49,0x49,0x41]),
    ('F', [0x7F,0x09,0x09,0x09,0x01]), ('G', [0x3E,0x41,0x49,0x49,0x7A]),
    ('H', [0x7F,0x08,0x08,0x08,0x7F]), ('I', [0x00,0x41,0x7F,0x41,0x00]),
    ('J', [0x20,0x40,0x41,0x3F,0x01]), ('K', [0x7F,0x08,0x14,0x22,0x41]),
    ('L', [0x7F,0x40,0x40,0x40,0x40]), ('M', [0x7F,0x02,0x0C,0x02,0x7F]),
    ('N', [0x7F,0x04,0x08,0x10,0x7F]), ('O', [0x3E,0x41,0x41,0x41,0x3E]),
    ('P', [0x7F,0x09,0x09,0x09,0x06]), ('Q', [0x3E,0x41,0x51,0x21,0x5E]),
    ('R', [0x7F,0x09,0x19,0x29,0x46]), ('S', [0x46,0x49,0x49,0x49,0x31]),
    ('T', [0x01,0x01,0x7F,0x01,0x01]), ('U', [0x3F,0x40,0x40,0x40,0x3F]),
    ('V', [0x1F,0x20,0x40,0x20,0x1F]), ('W', [0x7F,0x20,0x18,0x20,0x7F]),
    ('X', [0x63,0x14,0x08,0x14,0x63]), ('Y', [0x03,0x04,0x78,0x04,0x03]),
    ('Z', [0x61,0x51,0x49,0x45,0x43]), ('[', [0x00,0x7F,0x41,0x41,0x00]),
    ('\\',[0x02,0x04,0x08,0x10,0x20]), (']', [0x00,0x41,0x41,0x7F,0x00]),
    ('^', [0x04,0x02,0x01,0x02,0x04]), ('_', [0x40,0x40,0x40,0x40,0x40]),
    ('`', [0x00,0x01,0x02,0x04,0x00]), ('a', [0x20,0x54,0x54,0x54,0x78]),
    ('b', [0x7F,0x48,0x44,0x44,0x38]), ('c', [0x38,0x44,0x44,0x44,0x20]),
    ('d', [0x38,0x44,0x44,0x48,0x7F]), ('e', [0x38,0x54,0x54,0x54,0x18]),
    ('f', [0x08,0x7E,0x09,0x01,0x02]), ('g', [0x0C,0x52,0x52,0x52,0x3E]),
    ('h', [0x7F,0x08,0x04,0x04,0x78]), ('i', [0x00,0x44,0x7D,0x40,0x00]),
    ('j', [0x20,0x40,0x44,0x3D,0x00]), ('k', [0x7F,0x10,0x28,0x44,0x00]),
    ('l', [0x00,0x41,0x7F,0x40,0x00]), ('m', [0x7C,0x04,0x18,0x04,0x78]),
    ('n', [0x7C,0x08,0x04,0x04,0x78]), ('o', [0x38,0x44,0x44,0x44,0x38]),
    ('p', [0x7C,0x14,0x14,0x14,0x08]), ('q', [0x08,0x14,0x14,0x18,0x7C]),
    ('r', [0x7C,0x08,0x04,0x04,0x08]), ('s', [0x48,0x54,0x54,0x54,0x20]),
    ('t', [0x04,0x3F,0x44,0x40,0x20]), ('u', [0x3C,0x40,0x40,0x20,0x7C]),
    ('v', [0x1C,0x20,0x40,0x20,0x1C]), ('w', [0x3C,0x40,0x30,0x40,0x3C]),
    ('x', [0x44,0x28,0x10,0x28,0x44]), ('y', [0x0C,0x50,0x50,0x50,0x3C]),
    ('z', [0x44,0x64,0x54,0x4C,0x44]), ('{', [0x00,0x08,0x36,0x41,0x00]),
    ('|', [0x00,0x00,0x7F,0x00,0x00]), ('}', [0x00,0x41,0x36,0x08,0x00]),
    ('~', [0x08,0x04,0x08,0x10,0x08]), ('\u{7f}', [0x7F,0x41,0x41,0x41,0x7F]),
];

/// Glyph columns for `c`, folding the handful of non-ASCII characters the card
/// actually uses onto something drawable. A `·` is the card's own separator and
/// an accented vowel is unavoidable in Italian, so both get a shape rather than
/// the missing-glyph box.
fn glyph(c: char) -> [u8; 5] {
    let c = match c {
        'à' | 'á' | 'â' => 'a',
        'è' | 'é' | 'ê' => 'e',
        'ì' | 'í' => 'i',
        'ò' | 'ó' | 'ô' => 'o',
        'ù' | 'ú' => 'u',
        '·' | '•' => '.',
        '…' => '.',
        '±' => '+',
        '→' => '>',
        '≈' | '~' => '~',
        '✓' => '+',
        other => other,
    };
    for (k, g) in FONT.iter() {
        if *k == c {
            return *g;
        }
    }
    [0x7F, 0x41, 0x41, 0x41, 0x7F] // box: a glyph we do not have, made visible
}

/// Width in pixels of `s` drawn at `scale` (one blank column between glyphs).
pub fn text_width(s: &str, scale: usize) -> usize {
    let n = s.chars().count();
    if n == 0 {
        0
    } else {
        (n * (GLYPH_W + 1) - 1) * scale
    }
}

/// An RGB8 image being drawn into.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, bg: [u8; 3]) -> Canvas {
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            px.extend_from_slice(&bg);
        }
        Canvas { w, h, px }
    }

    #[inline]
    fn put(&mut self, x: isize, y: isize, c: [u8; 3]) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 3;
        self.px[i..i + 3].copy_from_slice(&c);
    }

    pub fn fill_rect(&mut self, x: isize, y: isize, w: usize, h: usize, c: [u8; 3]) {
        for dy in 0..h as isize {
            for dx in 0..w as isize {
                self.put(x + dx, y + dy, c);
            }
        }
    }

    /// A one-pixel-thick (times `t`) rectangle outline.
    pub fn rect_outline(&mut self, x: isize, y: isize, w: usize, h: usize, t: usize, c: [u8; 3]) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h as isize - t as isize, w, t, c);
        self.fill_rect(x, y, t, h, c);
        self.fill_rect(x + w as isize - t as isize, y, t, h, c);
    }

    /// Draw `s` with its top-left at (x, y), each font pixel a `scale` square.
    pub fn text(&mut self, x: isize, y: isize, scale: usize, c: [u8; 3], s: &str) {
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (col, bits) in g.iter().enumerate() {
                for row in 0..GLYPH_H {
                    if bits & (1 << row) != 0 {
                        self.fill_rect(
                            cx + (col * scale) as isize,
                            y + (row * scale) as isize,
                            scale,
                            scale,
                            c,
                        );
                    }
                }
            }
            cx += ((GLYPH_W + 1) * scale) as isize;
        }
    }

    /// Same, centred on `cx`.
    pub fn text_centered(&mut self, cx: isize, y: isize, scale: usize, c: [u8; 3], s: &str) {
        self.text(cx - (text_width(s, scale) / 2) as isize, y, scale, c, s);
    }

    /// Encode as a PNG byte stream.
    pub fn to_png(&self) -> Vec<u8> {
        encode(self.w, self.h, 3, &self.px)
    }
}

/// Encode raw 8-bit samples as a PNG. `channels` is 3 (RGB) or 4 (RGBA) — the
/// icon needs transparent corners, the Wrapped card does not.
pub fn encode(w: usize, h: usize, channels: usize, px: &[u8]) -> Vec<u8> {
    let stride = w * channels;
    // PNG filtering, per scanline. `Up` turns an identical row into a run of
    // zeroes, which is most of a flat-coloured image — and a zero run is
    // exactly what the matcher in `deflate_fixed` collapses best.
    let mut raw = Vec::with_capacity((stride + 1) * h);
    for y in 0..h {
        let row = &px[y * stride..(y + 1) * stride];
        if y == 0 {
            raw.push(0); // None: nothing above to subtract
            raw.extend_from_slice(row);
        } else {
            raw.push(2); // Up
            let prev = &px[(y - 1) * stride..y * stride];
            for i in 0..stride {
                raw.push(row[i].wrapping_sub(prev[i]));
            }
        }
    }

    let mut out = Vec::with_capacity(raw.len() / 8 + 1024);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    // colour type 2 = truecolour, 6 = truecolour with alpha
    ihdr.extend_from_slice(&[8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// zlib stream around one fixed-Huffman DEFLATE block.
fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // deflate, 32K window, no preset dict
    out.extend_from_slice(&deflate_fixed(raw));
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Writes bits LSB-first, the order DEFLATE uses for everything except the
/// Huffman codes themselves (which are MSB-first — see `huff`).
struct Bits {
    out: Vec<u8>,
    cur: u32,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits { out: Vec::new(), cur: 0, n: 0 }
    }
    fn push(&mut self, val: u32, bits: u32) {
        self.cur |= val << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push((self.cur & 0xFF) as u8);
            self.cur >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code, whose bits travel most-significant first.
    fn huff(&mut self, code: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.push((code >> i) & 1, 1);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.cur & 0xFF) as u8);
        }
        self.out
    }
}

/// The fixed literal/length code of RFC 1951 §3.2.6.
fn lit_code(sym: u32) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + (sym - 144), 9),
        256..=279 => (sym - 256, 7),
        _ => (0xC0 + (sym - 280), 8),
    }
}

/// Length symbol, extra bits and extra value for a match of `len` (3..=258).
fn len_sym(len: u32) -> (u32, u32, u32) {
    const BASE: [u32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    let mut i = 28;
    while i > 0 && BASE[i] > len {
        i -= 1;
    }
    (257 + i as u32, EXTRA[i], len - BASE[i])
}

/// One fixed-Huffman block. The matcher only ever looks one byte back: after
/// `Up` filtering the data is dominated by runs of a single value, and a
/// distance-1 match of length L expands into exactly that run — which is where
/// essentially all of the compression on a flat card comes from. Anything
/// cleverer would cost code for a case this image does not have.
fn deflate_fixed(raw: &[u8]) -> Vec<u8> {
    let mut b = Bits::new();
    b.push(1, 1); // final block
    b.push(1, 2); // fixed Huffman
    let mut i = 0usize;
    while i < raw.len() {
        // How far does the byte at i repeat?
        let mut run = 1usize;
        while i + run < raw.len() && raw[i + run] == raw[i] && run < 259 {
            run += 1;
        }
        // A match needs a literal in front of it to copy from, and must be >= 3.
        if run >= 4 {
            let (c, n) = lit_code(raw[i] as u32);
            b.huff(c, n);
            let mut left = (run - 1) as u32;
            while left >= 3 {
                let take = left.min(258);
                let (sym, extra, val) = len_sym(take);
                let (c, n) = lit_code(sym);
                b.huff(c, n);
                if extra > 0 {
                    b.push(val, extra);
                }
                b.huff(0, 5); // distance code 0 = distance 1
                left -= take;
            }
            for _ in 0..left {
                let (c, n) = lit_code(raw[i] as u32);
                b.huff(c, n);
            }
            i += run;
        } else {
            let (c, n) = lit_code(raw[i] as u32);
            b.huff(c, n);
            i += 1;
        }
    }
    let (c, n) = lit_code(256); // end of block
    b.huff(c, n);
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_header_and_chunks_are_well_formed() {
        let c = Canvas::new(8, 4, [10, 20, 30]);
        let p = c.to_png();
        assert_eq!(&p[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        // IHDR: length 13, then the name, then width/height
        assert_eq!(&p[8..12], &13u32.to_be_bytes());
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(&p[16..20], &8u32.to_be_bytes());
        assert_eq!(&p[20..24], &4u32.to_be_bytes());
        assert_eq!(p[24], 8, "8 bit per canale");
        assert_eq!(p[25], 2, "truecolour RGB");
        assert!(p.ends_with(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]), "IEND + il suo CRC");
    }

    #[test]
    fn crc_and_adler_match_known_values() {
        // Valori di riferimento: IEND vuoto ha sempre lo stesso CRC, ed e' il
        // controllo piu' rapido che l'implementazione non sia sfasata.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"abc"), 0x024D_0127);
    }

    /// Riporta i byte compressi al chiaro, per verificare che il flusso DEFLATE
    /// dica davvero quello che credevamo di scrivere. Gestisce solo il blocco a
    /// Huffman fisso che `deflate_fixed` produce.
    fn inflate_fixed(data: &[u8]) -> Vec<u8> {
        let (mut pos, mut bit) = (0usize, 0u32);
        let mut take = |n: u32| -> u32 {
            let mut v = 0;
            for i in 0..n {
                let b = (data[pos] >> bit) & 1;
                v |= (b as u32) << i;
                bit += 1;
                if bit == 8 {
                    bit = 0;
                    pos += 1;
                }
            }
            v
        };
        assert_eq!(take(1), 1, "blocco finale");
        assert_eq!(take(2), 1, "Huffman fisso");
        let mut out: Vec<u8> = Vec::new();
        loop {
            // decodifica di un simbolo letterale/lunghezza, MSB per primo
            let mut code = take(1);
            let mut len = 1;
            let sym = loop {
                match (len, code) {
                    (7, c) if c <= 0b0010111 => break 256 + c,
                    (8, c) if (0b00110000..=0b10111111).contains(&c) => break c - 0x30,
                    // solo 0xC0..0xC7: da 0xC8 in su sono i primi 8 bit di un
                    // codice a 9, e fermarsi qui li scambierebbe per simboli
                    (8, c) if (0xC0..=0xC7).contains(&c) => break 280 + (c - 0xC0),
                    (9, c) => break 144 + (c - 0x190),
                    _ => {}
                }
                code = (code << 1) | take(1);
                len += 1;
                assert!(len <= 9, "codice non valido");
            };
            if sym == 256 {
                break;
            }
            if sym < 256 {
                out.push(sym as u8);
            } else {
                const BASE: [u32; 29] = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258];
                const EXTRA: [u32; 29] = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0];
                let i = (sym - 257) as usize;
                let l = BASE[i] + if EXTRA[i] > 0 { take(EXTRA[i]) } else { 0 };
                let mut dcode = take(1);
                for _ in 0..4 {
                    dcode = (dcode << 1) | take(1);
                }
                assert_eq!(dcode, 0, "solo distanza 1");
                for _ in 0..l {
                    let b = out[out.len() - 1];
                    out.push(b);
                }
            }
        }
        out
    }

    #[test]
    fn what_we_compress_is_what_comes_back() {
        for case in [
            vec![],
            vec![7u8],
            vec![0u8; 5000],                       // la corsa lunga: il caso che conta
            (0..=255u8).collect::<Vec<u8>>(),      // nessuna ripetizione
            {
                let mut v = vec![1u8, 2, 3];
                v.extend(std::iter::repeat(9).take(300)); // corsa oltre i 258 di un match
                v.extend([4u8, 5]);
                v
            },
        ] {
            assert_eq!(inflate_fixed(&deflate_fixed(&case)), case, "len {}", case.len());
        }
    }

    #[test]
    fn a_flat_card_does_not_weigh_two_megabytes() {
        let mut c = Canvas::new(1200, 630, [5, 17, 12]);
        c.fill_rect(40, 40, 400, 120, [10, 32, 24]);
        c.text(60, 60, 4, [57, 217, 138], "PHOSPHOR 2026");
        let p = c.to_png();
        let raw = 1200 * 630 * 3;
        assert!(p.len() < raw / 20, "compresso: {} byte contro {} grezzi", p.len(), raw);
    }

    #[test]
    fn glyphs_are_the_letters_they_claim_to_be() {
        // Il rischio di un font scritto a mano e' che una lettera sia storta e
        // nessuno se ne accorga: qui si guarda la forma, non solo che ci sia.
        let mut c = Canvas::new(12, 7, [0, 0, 0]);
        c.text(0, 0, 1, [255, 255, 255], "AI");
        let art: Vec<String> = (0..7)
            .map(|y| {
                (0..12)
                    .map(|x| if c.px[(y * 12 + x) * 3] > 0 { '#' } else { '.' })
                    .collect()
            })
            .collect();
        assert_eq!(art[0], ".###...###..", "cappello della A, grazia alta della I");
        assert_eq!(art[4], "#####...#...", "la traversa della A, l'asta della I");
        assert_eq!(art[6], "#...#..###..", "le gambe della A, grazia bassa della I");
    }
}
