//! The application icon, drawn in code and encoded as a Windows `.ico`.
//!
//! The subject is **PHOS**, the little CRT that already lives in the TUI beside
//! the banner — two eyes and a smile on a phosphor-green screen. A face is the
//! right choice for an icon because a face survives being tiny: at 16×16 there
//! are barely 256 pixels to work with, and people recognise two dots and a
//! curve long before they recognise a logo.
//!
//! Drawn as **pixel art at 16×16 and scaled by whole numbers** (×2 ×3 ×4 ×8
//! ×16) rather than designed large and shrunk. Downscaling is what turns a
//! crisp icon into grey mush at small sizes; this way the smallest size is the
//! one that was actually designed, and every larger size is exactly it, only
//! bigger — which is also the pixel aesthetic the app already has.
//!
//! No image crate: the frames go through [`crate::png`], and the ICO container
//! is a hundred lines of header. See [`ico`].

use crate::png;

/// Transparent, screen interior, bezel/features. One char per pixel, so the
/// artwork is legible in the source and a change is visible in the diff.
const ART: [&str; 16] = [
    "................",
    "................",
    ".##############.",
    ".#::::::::::::#.",
    ".#::::::::::::#.",
    ".#::##::::##::#.",
    ".#::##::::##::#.",
    ".#::::::::::::#.",
    ".#::#::::::#::#.",
    ".#:::######:::#.",
    ".#::::::::::::#.",
    ".##############.",
    ".....######.....",
    "...##########...",
    "................",
    "................",
];

const MASTER: usize = 16;

/// The phosphor palette, shared with the Wrapped card so the app and its icon
/// are visibly the same product.
const BEZEL: [u8; 4] = [0x39, 0xd9, 0x8a, 0xff]; // green, the silhouette
const FEATURE: [u8; 4] = [0x7d, 0xff, 0xb0, 0xff]; // brighter: eyes and smile
const SCREEN: [u8; 4] = [0x05, 0x19, 0x11, 0xff]; // near-black, faintly green
const SCANLINE: [u8; 4] = [0x0a, 0x2a, 0x1e, 0xff]; // the CRT's texture
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// Sizes Windows actually asks for: 16 in menus and the taskbar, 32 on the
/// desktop at 100%, 48 at 150%, 256 in the large-icon view and the file
/// dialog's preview. All whole multiples of the 16×16 master.
pub const SIZES: [usize; 6] = [16, 32, 48, 64, 128, 256];

/// Is this pixel one of the eyes or the mouth? Those get the brighter colour,
/// which is what makes the face read once the bezel is no longer the only
/// green thing on screen.
fn is_feature(x: usize, y: usize) -> bool {
    matches!(y, 5 | 6 | 8 | 9) && (2..14).contains(&x)
}

/// Render the icon at `size` (a multiple of 16) as RGBA.
pub fn render(size: usize) -> Vec<u8> {
    let scale = (size / MASTER).max(1);
    let mut px = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (sx, sy) = (x / scale, y / scale);
            let ch = ART[sy.min(MASTER - 1)]
                .as_bytes()
                .get(sx)
                .copied()
                .unwrap_or(b'.');
            let mut c = match ch {
                b'#' if is_feature(sx, sy) => FEATURE,
                b'#' => BEZEL,
                b':' => SCREEN,
                _ => CLEAR,
            };
            // Scanlines, inside the screen only. The period follows the SCALE —
            // one line per source row — so the texture looks the same at every
            // size instead of turning into a fine screen door at 256. Below
            // 48px there are too few device rows: the lines would eat the face
            // rather than decorate it.
            if ch == b':' && size >= 48 {
                let thickness = (scale / 8).max(1);
                if y % scale >= scale - thickness {
                    c = SCANLINE;
                }
            }
            let i = (y * size + x) * 4;
            px[i..i + 4].copy_from_slice(&c);
        }
    }
    px
}

/// One `.ico` holding every size in [`SIZES`].
///
/// Small frames go in as classic BMP (a bottom-up DIB plus the 1-bit AND mask
/// the format still demands); 128 and 256 go in as PNG, which every Windows
/// since Vista reads and which keeps the file at tens of KB instead of a third
/// of a megabyte.
pub fn ico() -> Vec<u8> {
    let frames: Vec<(usize, Vec<u8>)> = SIZES
        .iter()
        .map(|&s| {
            let rgba = render(s);
            (s, if s >= 128 { png::encode(s, s, 4, &rgba) } else { bmp_frame(s, &rgba) })
        })
        .collect();

    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon
    out.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    // The directory is fixed-width, so every offset is known before any pixel
    // data is written.
    let mut offset = 6 + frames.len() * 16;
    for (size, data) in &frames {
        // 256 does not fit in a byte and is encoded as 0 — the one quirk of the
        // format that silently truncates an icon to nothing if you miss it.
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(dim);
        out.push(dim);
        out.push(0); // palette size: 0 = truecolour
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += data.len();
    }
    for (_, data) in &frames {
        out.extend_from_slice(data);
    }
    out
}

/// A BITMAPINFOHEADER + BGRA pixels + AND mask, as an icon frame wants it.
fn bmp_frame(size: usize, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + size * size * 4 + size * size / 8);
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(size as i32).to_le_bytes());
    // DOUBLE height: the header covers the colour image and the mask together.
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, uncompressed
    out.extend_from_slice(&((size * size * 4) as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // resolution and palette counts: unused

    // Colour data, bottom-up, BGRA.
    for y in (0..size).rev() {
        for x in 0..size {
            let i = (y * size + x) * 4;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // AND mask: 1 = leave the background alone. Redundant next to the alpha
    // channel on anything modern, but a missing or wrong mask still shows up as
    // a black box in some shell paths, so it is built properly. Rows are padded
    // to 4 bytes, bottom-up like the colour data.
    let row_bytes = size.div_ceil(32) * 4;
    for y in (0..size).rev() {
        let mut row = vec![0u8; row_bytes];
        for x in 0..size {
            if rgba[(y * size + x) * 4 + 3] == 0 {
                row[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_artwork_is_a_square_and_symmetric() {
        // Un volto storto si nota subito ma solo guardandolo: qui lo si prende
        // senza guardare.
        assert_eq!(ART.len(), MASTER);
        for (y, row) in ART.iter().enumerate() {
            assert_eq!(row.len(), MASTER, "riga {y}");
            let b = row.as_bytes();
            for x in 0..MASTER {
                assert_eq!(b[x], b[MASTER - 1 - x], "riga {y} non e' simmetrica in {x}");
            }
        }
    }

    #[test]
    fn scaling_keeps_the_pixels_square() {
        // 32 e' esattamente il master raddoppiato: ogni pixel sorgente diventa
        // un quadrato 2x2 dello stesso colore (a meno delle scanline, che
        // toccano solo l'interno dello schermo).
        let px = render(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| {
            let i = (y * 32 + x) * 4;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // l'angolo in alto a sinistra e' trasparente, come nell'arte
        assert_eq!(at(0, 0), CLEAR);
        // il bezel del master (2,2) copre i device pixel 4..6 x 4..6
        for y in 4..6 {
            for x in 4..6 {
                assert_eq!(at(x, y), BEZEL, "bezel in {x},{y}");
            }
        }
    }

    #[test]
    fn the_face_is_brighter_than_the_frame() {
        // Se occhi e bocca finissero dello stesso verde del bordo, l'icona
        // diventerebbe un rettangolo e basta.
        let px = render(16);
        let at = |x: usize, y: usize| {
            let i = (y * 16 + x) * 4;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(4, 5), FEATURE, "occhio sinistro");
        assert_eq!(at(11, 6), FEATURE, "occhio destro");
        assert_eq!(at(7, 9), FEATURE, "bocca");
        assert_eq!(at(1, 3), BEZEL, "montante laterale");
        assert_ne!(FEATURE, BEZEL);
    }

    #[test]
    fn the_ico_directory_points_where_it_claims() {
        let data = ico();
        assert_eq!(&data[0..2], &[0, 0], "riservato");
        assert_eq!(&data[2..4], &[1, 0], "tipo: icona");
        let n = u16::from_le_bytes([data[4], data[5]]) as usize;
        assert_eq!(n, SIZES.len());
        for i in 0..n {
            let e = 6 + i * 16;
            let expected = SIZES[i];
            let dim = data[e] as usize;
            assert_eq!(dim, if expected >= 256 { 0 } else { expected }, "lato del frame {i}");
            let len = u32::from_le_bytes(data[e + 8..e + 12].try_into().unwrap()) as usize;
            let off = u32::from_le_bytes(data[e + 12..e + 16].try_into().unwrap()) as usize;
            assert!(off + len <= data.len(), "il frame {i} finisce fuori dal file");
            let frame = &data[off..off + len];
            if expected >= 128 {
                assert_eq!(&frame[..4], &[0x89, b'P', b'N', b'G'], "frame {expected} e' PNG");
            } else {
                // BITMAPINFOHEADER, con l'altezza DOPPIA per via della maschera
                assert_eq!(u32::from_le_bytes(frame[..4].try_into().unwrap()), 40);
                assert_eq!(i32::from_le_bytes(frame[4..8].try_into().unwrap()), expected as i32);
                assert_eq!(
                    i32::from_le_bytes(frame[8..12].try_into().unwrap()),
                    (expected * 2) as i32,
                    "l'altezza del frame {expected} deve contare anche la maschera"
                );
            }
        }
    }
}
