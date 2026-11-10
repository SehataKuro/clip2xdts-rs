//! TrackActionMixer の "cmt 01x0 binc" 形式（celsysdocument）のパーサー
//!
//! 形式（すべてリトルエンディアン）:
//!   "cmt 0110" "binc" u32(不明) u32(文字列数) [u8 長さ + UTF-8]...
//!   ノード: u32 16, u32 値の終端, u32 属性の終端, u32 名前, u32 型, 値...,
//!           u32 属性数, (u32 キー, u32 値)..., u32 子の数, 子ノード...

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Val {
    None,
    Str(String),
    F64(f64),
    I64(i64),
    F64s(Vec<f64>),
    I32s(Vec<i32>),
    Bytes(Vec<u8>),
    Strs(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub val: Val,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        if self.name == name {
            out.push(self);
        }
        for k in &self.kids {
            k.find_all(name, out);
        }
    }
    pub fn f64(&self) -> Option<f64> {
        match &self.val {
            Val::F64(v) => Some(*v),
            Val::I64(v) => Some(*v as f64),
            _ => None,
        }
    }
}

struct P<'a> {
    d: &'a [u8],
    s: Vec<String>,
}

impl<'a> P<'a> {
    fn u32(&self, q: usize) -> Result<u32, String> {
        self.d
            .get(q..q + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "mixer: データが途中で終わっています".to_string())
    }
    fn str(&self, i: u32) -> Result<String, String> {
        self.s
            .get(i as usize)
            .cloned()
            .ok_or_else(|| "mixer: 文字列番号が範囲外です".to_string())
    }
    fn f64(&self, q: usize) -> Result<f64, String> {
        let b = self.d.get(q..q + 8).ok_or("mixer: データが途中で終わっています")?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(f64::from_le_bytes(a))
    }

    fn node(&self, q: usize, depth: usize) -> Result<(Node, usize), String> {
        if depth > 256 {
            return Err("mixer: 入れ子が深すぎます".into());
        }
        let x = self.u32(q + 4)? as usize;
        let y = self.u32(q + 8)? as usize;
        let name = self.str(self.u32(q + 12)?)?;
        let tn = self.str(self.u32(q + 16)?)?;
        let v = q + 20;
        let val = match tn.as_str() {
            "String" => Val::Str(self.str(self.u32(v)?)?),
            "Double" => Val::F64(self.f64(v)?),
            "Int32" => Val::I64(self.u32(v)? as i32 as i64),
            "UInt32" => Val::I64(self.u32(v)? as i64),
            "Byte" => Val::I64(*self.d.get(v).ok_or("mixer: 範囲外")? as i64),
            "Double[]" => {
                let c = self.u32(v)? as usize;
                Val::F64s((0..c).map(|i| self.f64(v + 4 + 8 * i)).collect::<Result<_, _>>()?)
            }
            "Int32[]" => {
                let c = self.u32(v)? as usize;
                Val::I32s(
                    (0..c).map(|i| self.u32(v + 4 + 4 * i).map(|x| x as i32)).collect::<Result<_, _>>()?,
                )
            }
            "Byte[]" => {
                let c = self.u32(v)? as usize;
                Val::Bytes(self.d.get(v + 4..v + 4 + c).ok_or("mixer: 範囲外")?.to_vec())
            }
            "String[]" => {
                let c = self.u32(v)? as usize;
                Val::Strs(
                    (0..c)
                        .map(|i| self.u32(v + 4 + 4 * i).and_then(|k| self.str(k)))
                        .collect::<Result<_, _>>()?,
                )
            }
            _ => Val::None, // null と未対応の型（サイズはヘッダで読み飛ばせる）
        };
        let a = q + x;
        let ac = self.u32(a)? as usize;
        let mut attrs = Vec::with_capacity(ac);
        for i in 0..ac {
            attrs.push((self.str(self.u32(a + 4 + 8 * i)?)?, self.str(self.u32(a + 8 + 8 * i)?)?));
        }
        let mut c = q + y;
        let cc = self.u32(c)? as usize;
        c += 4;
        let mut kids = Vec::with_capacity(cc.min(1024));
        for _ in 0..cc {
            let (k, nc) = self.node(c, depth + 1)?;
            kids.push(k);
            c = nc;
        }
        Ok((Node { name, val, attrs, kids }, c))
    }
}

pub fn parse(d: &[u8]) -> Result<Node, String> {
    if d.len() < 20 || &d[..4] != b"cmt " || &d[8..12] != b"binc" {
        return Err("mixer: 未知の形式です".into());
    }
    let mut p = P { d, s: Vec::new() };
    let n = p.u32(16)? as usize;
    let mut q = 20;
    let mut s = Vec::with_capacity(n);
    for _ in 0..n {
        let l = *d.get(q).ok_or("mixer: 文字列表が途中で終わっています")? as usize;
        q += 1;
        let b = d.get(q..q + l).ok_or("mixer: 文字列表が途中で終わっています")?;
        s.push(String::from_utf8_lossy(b).into_owned());
        q += l;
    }
    p.s = s;
    Ok(p.node(q, 0)?.0)
}
