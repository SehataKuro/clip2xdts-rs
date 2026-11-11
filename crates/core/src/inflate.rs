//! 最小限の zlib / DEFLATE 展開（RFC 1950 / 1951）

pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 2 {
        return Err("zlib: データが短すぎます".into());
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0f != 8 || ((cmf as u16) << 8 | flg as u16) % 31 != 0 {
        return Err("zlib: ヘッダが不正です".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib: プリセット辞書には未対応です".into());
    }
    inflate(&data[2..])
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.nbits < n {
            if self.pos >= self.data.len() {
                return Err("deflate: データが途中で終わっています".into());
            }
            self.bit |= (self.data[self.pos] as u32) << self.nbits;
            self.pos += 1;
            self.nbits += 8;
        }
        Ok(())
    }
    fn get(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }
    fn align(&mut self) {
        self.bit = 0;
        self.nbits = 0;
    }
}

struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    fn decode(&self, b: &mut Bits) -> Result<u16, String> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("deflate: 不正なハフマン符号".into())
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut b = Bits { data, pos: 0, bit: 0, nbits: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(data.len() * 4);
    loop {
        let last = b.get(1)?;
        let btype = b.get(2)?;
        match btype {
            0 => {
                b.align();
                if b.pos + 4 > data.len() {
                    return Err("deflate: 非圧縮ブロックが不正です".into());
                }
                let len = u16::from_le_bytes([data[b.pos], data[b.pos + 1]]) as usize;
                b.pos += 4;
                if b.pos + len > data.len() {
                    return Err("deflate: 非圧縮ブロックが途中で終わっています".into());
                }
                out.extend_from_slice(&data[b.pos..b.pos + len]);
                b.pos += len;
            }
            1 | 2 => {
                let (lit, dist) = if btype == 1 {
                    let mut l = [0u8; 288];
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    (Huffman::new(&l), Huffman::new(&[5u8; 30]))
                } else {
                    dynamic_tables(&mut b)?
                };
                loop {
                    let sym = lit.decode(&mut b)? as usize;
                    if sym < 256 {
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let i = sym - 257;
                        if i >= 29 {
                            return Err("deflate: 不正な長さ符号".into());
                        }
                        let len = LEN_BASE[i] as usize + b.get(LEN_EXTRA[i] as u32)? as usize;
                        let di = dist.decode(&mut b)? as usize;
                        if di >= 30 {
                            return Err("deflate: 不正な距離符号".into());
                        }
                        let d = DIST_BASE[di] as usize + b.get(DIST_EXTRA[di] as u32)? as usize;
                        if d > out.len() {
                            return Err("deflate: 距離が範囲外です".into());
                        }
                        let start = out.len() - d;
                        for k in 0..len {
                            let v = out[start + k];
                            out.push(v);
                        }
                    }
                }
            }
            _ => return Err("deflate: 不正なブロック種別".into()),
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

fn dynamic_tables(b: &mut Bits) -> Result<(Huffman, Huffman), String> {
    let hlit = b.get(5)? as usize + 257;
    let hdist = b.get(5)? as usize + 1;
    let hclen = b.get(4)? as usize + 4;
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut cl = [0u8; 19];
    for &o in ORDER.iter().take(hclen) {
        cl[o] = b.get(3)? as u8;
    }
    let clh = Huffman::new(&cl);
    let mut lens = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < hlit + hdist {
        let sym = clh.decode(b)?;
        match sym {
            0..=15 => {
                lens[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err("deflate: 繰り返し対象がありません".into());
                }
                let prev = lens[i - 1];
                let n = 3 + b.get(2)? as usize;
                for _ in 0..n {
                    if i >= lens.len() {
                        return Err("deflate: 符号長が多すぎます".into());
                    }
                    lens[i] = prev;
                    i += 1;
                }
            }
            17 | 18 => {
                let n = if sym == 17 { 3 + b.get(3)? } else { 11 + b.get(7)? } as usize;
                if i + n > lens.len() {
                    return Err("deflate: 符号長が多すぎます".into());
                }
                i += n;
            }
            _ => return Err("deflate: 不正な符号長".into()),
        }
    }
    Ok((Huffman::new(&lens[..hlit]), Huffman::new(&lens[hlit..])))
}
