//! A tiny PNG writer (stored deflate blocks), so the sample mailbox can carry real images without
//! an image library.

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &d in data {
        a = (a + d as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut c = kind.to_vec();
    c.extend_from_slice(data);
    out.extend_from_slice(&c);
    out.extend_from_slice(&crc32(&c).to_be_bytes());
}

/// Encode RGB pixels (`w * h * 3` bytes).
pub fn encode_rgb(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 3 + 1) as usize * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        let row = y * w as usize * 3;
        raw.extend_from_slice(rgb.get(row..row + w as usize * 3).unwrap_or(&[]));
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A banner: a horizontal gradient between two colours with a band of stripes, `w` × `h`.
pub fn banner_png(w: u32, h: u32, from: [u8; 3], to: [u8; 3]) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / w.max(1) as f32;
            let mut c = [0u8; 3];
            for i in 0..3 {
                c[i] = (from[i] as f32 * (1.0 - t) + to[i] as f32 * t) as u8;
            }
            let band = y > h / 3 && y < h * 2 / 3 && (x / 12) % 2 == 0;
            if band {
                c = [c[0].saturating_add(40), c[1].saturating_add(40), c[2].saturating_add(40)];
            }
            px.extend_from_slice(&c);
        }
    }
    encode_rgb(w, h, &px)
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_of_iend_is_the_known_value() {
        assert_eq!(super::crc32(b"IEND"), 0xAE42_6082);
    }
}
