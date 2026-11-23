//! A small PNG writer (RGBA8): zlib stream with one fixed-Huffman deflate block and a greedy LZ77
//! matcher (hash chains). Good enough for screenshots of flat user interfaces.

/// PNG file of an RGBA8 image.
pub fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * stride..(y + 1) * stride]);
    }
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib(&raw));
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

pub fn crc32(b: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &x in b {
        c = table[((c ^ x as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(b: &[u8]) -> u32 {
    let (mut a, mut s) = (1u32, 0u32);
    for chunk in b.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            s += a;
        }
        a %= 65521;
        s %= 65521;
    }
    s << 16 | a
}

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// a Huffman code (MSB first)
    fn code(&mut self, code: u32, len: u32) {
        let mut r = 0;
        for i in 0..len {
            r |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put(r, len);
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn lit(b: &mut Bits, v: u32) {
    match v {
        0..=143 => b.code(0x30 + v, 8),
        144..=255 => b.code(0x190 + (v - 144), 9),
        256..=279 => b.code(v - 256, 7),
        _ => b.code(0xC0 + (v - 280), 8),
    }
}

const LEN_BASE: [u32; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u32; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u32; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn put_match(b: &mut Bits, len: u32, dist: u32) {
    let li = LEN_BASE.iter().rposition(|&x| x <= len).unwrap();
    lit(b, 257 + li as u32);
    b.put(len - LEN_BASE[li], LEN_EXTRA[li]);
    let di = DIST_BASE.iter().rposition(|&x| x <= dist).unwrap();
    b.code(di as u32, 5);
    b.put(dist - DIST_BASE[di], DIST_EXTRA[di]);
}

/// Raw deflate, one fixed-Huffman block.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WIN: usize = 32768;
    const HBITS: usize = 15;
    let mut b = Bits { out: Vec::with_capacity(data.len() / 4), acc: 0, n: 0 };
    b.put(1, 1); // final
    b.put(1, 2); // fixed Huffman
    let mut head = vec![usize::MAX; 1 << HBITS];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |i: usize| -> usize {
        let v = (data[i] as u32) | (data[i + 1] as u32) << 8 | (data[i + 2] as u32) << 16;
        (v.wrapping_mul(0x9E37_79B1) >> (32 - HBITS)) as usize
    };
    let mut i = 0;
    let insert = |i: usize, head: &mut Vec<usize>, prev: &mut Vec<usize>| {
        if i + 2 < data.len() {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i;
        }
    };
    while i < data.len() {
        let mut best = (0usize, 0usize);
        if i + 2 < data.len() {
            let mut cand = head[hash(i)];
            let mut steps = 0;
            let max = (data.len() - i).min(258);
            while cand != usize::MAX && i - cand <= WIN && steps < 48 {
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - cand);
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                steps += 1;
            }
        }
        if best.0 >= 3 {
            put_match(&mut b, best.0 as u32, best.1 as u32);
            for k in 0..best.0 {
                insert(i + k, &mut head, &mut prev);
            }
            i += best.0;
        } else {
            lit(&mut b, data[i] as u32);
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
    lit(&mut b, 256);
    b.finish()
}

pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(&deflate(data));
    z.extend_from_slice(&adler32(data).to_be_bytes());
    z
}

/// Inflates one fixed-Huffman deflate block (what [`deflate`] writes).
pub fn inflate_fixed(d: &[u8]) -> Vec<u8> {
    let mut pos = 0usize;
    let bit = |pos: &mut usize| -> u32 {
        let v = (d[*pos / 8] >> (*pos % 8)) & 1;
        *pos += 1;
        v as u32
    };
    let bits = |pos: &mut usize, n: u32| -> u32 {
        let mut v = 0;
        for k in 0..n {
            v |= bit(pos) << k;
        }
        v
    };
    let code = |pos: &mut usize, n: u32| -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = v << 1 | bit(pos);
        }
        v
    };
    assert_eq!(bits(&mut pos, 1), 1);
    assert_eq!(bits(&mut pos, 2), 1);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let mut c = code(&mut pos, 7);
        let sym = if c <= 0x17 {
            c + 256
        } else {
            c = c << 1 | bit(&mut pos);
            if (0x30..=0xBF).contains(&c) {
                c - 0x30
            } else if (0xC0..=0xC7).contains(&c) {
                c - 0xC0 + 280
            } else {
                c = c << 1 | bit(&mut pos);
                c - 0x190 + 144
            }
        };
        match sym {
            0..=255 => out.push(sym as u8),
            256 => break,
            _ => {
                let li = (sym - 257) as usize;
                let len = LEN_BASE[li] + bits(&mut pos, LEN_EXTRA[li]);
                let di = code(&mut pos, 5) as usize;
                let dist = DIST_BASE[di] + bits(&mut pos, DIST_EXTRA[di]);
                for _ in 0..len {
                    out.push(out[out.len() - dist as usize]);
                }
            }
        }
    }
    out
}


/// Decodes a PNG written by [`encode_rgba`] (RGBA8, filter 0, fixed-Huffman zlib): (w, h, rgba).
pub fn decode_own(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if png.get(..8)? != [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return None;
    }
    let (mut i, mut w, mut h, mut idat) = (8usize, 0u32, 0u32, Vec::new());
    while i + 8 <= png.len() {
        let n = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let kind = &png[i + 4..i + 8];
        let body = png.get(i + 8..i + 8 + n)?;
        if kind == b"IHDR" {
            w = u32::from_be_bytes(body[0..4].try_into().ok()?);
            h = u32::from_be_bytes(body[4..8].try_into().ok()?);
        } else if kind == b"IDAT" {
            idat.extend_from_slice(body);
        }
        i += 12 + n;
    }
    let raw = inflate_fixed(idat.get(2..idat.len().checked_sub(4)?)?);
    let stride = w as usize * 4;
    let mut out = Vec::with_capacity(stride * h as usize);
    for y in 0..h as usize {
        out.extend_from_slice(raw.get(y * (stride + 1) + 1..(y + 1) * (stride + 1))?);
    }
    Some((w, h, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deflate_round_trips() {
        let mut data = Vec::new();
        for i in 0..20000u32 {
            data.push((i % 7) as u8);
            if i % 1000 < 400 {
                data.push(200);
            } else {
                data.push((i * 31 % 251) as u8);
            }
        }
        let z = deflate(&data);
        assert!(z.len() < data.len() / 2, "{} of {}", z.len(), data.len());
        assert_eq!(inflate_fixed(&z), data);
        assert_eq!(inflate_fixed(&deflate(b"")), b"");
        assert_eq!(inflate_fixed(&deflate(b"ab")), b"ab");
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}
