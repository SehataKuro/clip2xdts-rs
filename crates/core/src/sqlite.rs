//! 読み取り専用の最小 SQLite リーダー（テーブルの全行スキャンのみ）

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Real(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
    /// TEXT/BLOB をバイト列として返す
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Text(s) => Some(s.as_bytes()),
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }
}

pub struct Row {
    cols: std::rc::Rc<HashMap<String, usize>>,
    vals: Vec<Value>,
}

static NULL: Value = Value::Null;

impl Row {
    pub fn get(&self, name: &str) -> &Value {
        self.cols
            .get(name)
            .and_then(|&i| self.vals.get(i))
            .unwrap_or(&NULL)
    }
    pub fn int(&self, name: &str) -> i64 {
        self.get(name).as_i64().unwrap_or(0)
    }
}

pub struct Db<'a> {
    data: &'a [u8],
    page_size: usize,
    usable: usize,
    tables: HashMap<String, (u32, Vec<String>)>,
}

impl<'a> Db<'a> {
    pub fn open(data: &'a [u8]) -> Result<Db<'a>, String> {
        if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
            return Err("SQLite データベースではありません".into());
        }
        let ps = u16::from_be_bytes([data[16], data[17]]) as usize;
        let page_size = if ps == 1 { 65536 } else { ps };
        let usable = page_size - data[20] as usize;
        let enc = u32::from_be_bytes([data[56], data[57], data[58], data[59]]);
        if enc > 1 {
            return Err("UTF-8 以外の SQLite には未対応です".into());
        }
        let mut db = Db { data, page_size, usable, tables: HashMap::new() };
        let mut tables = HashMap::new();
        let mut recs = Vec::new();
        db.scan(1, &mut recs)?;
        for (_rowid, vals) in recs {
            // sqlite_master: type, name, tbl_name, rootpage, sql
            if vals.first().and_then(|v| v.as_str()) != Some("table") {
                continue;
            }
            let name = vals.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let root = vals.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as u32;
            let sql = vals.get(4).and_then(|v| v.as_str()).unwrap_or("");
            tables.insert(name, (root, parse_columns(sql)));
        }
        db.tables = tables;
        Ok(db)
    }

    pub fn has_table(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    /// テーブルの全行を読む。INTEGER PRIMARY KEY 列には rowid を入れる
    pub fn rows(&self, table: &str) -> Result<Vec<Row>, String> {
        let (root, cols) = self
            .tables
            .get(table)
            .ok_or_else(|| format!("テーブル {} がありません", table))?;
        let map: HashMap<String, usize> =
            cols.iter().enumerate().map(|(i, c)| (c.clone(), i)).collect();
        let map = std::rc::Rc::new(map);
        let mut recs = Vec::new();
        self.scan(*root, &mut recs)?;
        Ok(recs
            .into_iter()
            .map(|(rowid, mut vals)| {
                if let Some(Value::Null) | None = vals.first() {
                    if vals.is_empty() {
                        vals.push(Value::Int(rowid));
                    } else {
                        vals[0] = Value::Int(rowid);
                    }
                }
                Row { cols: map.clone(), vals }
            })
            .collect())
    }

    fn page(&self, n: u32) -> Result<&'a [u8], String> {
        let start = (n as usize - 1) * self.page_size;
        let end = start + self.page_size;
        if n == 0 || end > self.data.len() {
            return Err(format!("SQLite: ページ {} が範囲外です", n));
        }
        Ok(&self.data[start..end])
    }

    fn scan(&self, pgno: u32, out: &mut Vec<(i64, Vec<Value>)>) -> Result<(), String> {
        let mut stack = vec![pgno];
        let mut guard = 0usize;
        while let Some(p) = stack.pop() {
            guard += 1;
            if guard > 10_000_000 {
                return Err("SQLite: B-tree が循環しています".into());
            }
            let page = self.page(p)?;
            let h = if p == 1 { 100 } else { 0 };
            let kind = page[h];
            let ncells = u16::from_be_bytes([page[h + 3], page[h + 4]]) as usize;
            match kind {
                0x05 => {
                    let right = be32(&page[h + 8..]);
                    let mut kids = Vec::with_capacity(ncells + 1);
                    for i in 0..ncells {
                        let off = be16(&page[h + 12 + 2 * i..]) as usize;
                        kids.push(be32(&page[off..]));
                    }
                    kids.push(right);
                    // 昇順に処理するため逆順で積む
                    for k in kids.into_iter().rev() {
                        stack.push(k);
                    }
                }
                0x0d => {
                    for i in 0..ncells {
                        let off = be16(&page[h + 8 + 2 * i..]) as usize;
                        let (plen, n1) = varint(&page[off..]);
                        let (rowid, n2) = varint(&page[off + n1..]);
                        let payload = self.payload(page, off + n1 + n2, plen as usize)?;
                        out.push((rowid as i64, decode_record(&payload)?));
                    }
                }
                _ => return Err(format!("SQLite: 未対応のページ種別 {:#x}", kind)),
            }
        }
        Ok(())
    }

    fn payload(&self, page: &[u8], at: usize, plen: usize) -> Result<Vec<u8>, String> {
        let u = self.usable;
        let x = u - 35;
        if plen <= x {
            return Ok(page[at..at + plen].to_vec());
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((plen - m) % (u - 4));
        let local = if k <= x { k } else { m };
        let mut buf = Vec::with_capacity(plen);
        buf.extend_from_slice(&page[at..at + local]);
        let mut next = be32(&page[at + local..]);
        while buf.len() < plen {
            if next == 0 {
                return Err("SQLite: オーバーフローページが途切れています".into());
            }
            let op = self.page(next)?;
            next = be32(op);
            let take = (plen - buf.len()).min(u - 4);
            buf.extend_from_slice(&op[4..4 + take]);
        }
        Ok(buf)
    }
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn varint(b: &[u8]) -> (u64, usize) {
    let mut v: u64 = 0;
    for i in 0..8 {
        let c = b[i];
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return (v, i + 1);
        }
    }
    v = (v << 8) | b[8] as u64;
    (v, 9)
}

fn decode_record(p: &[u8]) -> Result<Vec<Value>, String> {
    let (hlen, mut hp) = varint(p);
    let hlen = hlen as usize;
    let mut types = Vec::new();
    while hp < hlen {
        let (t, n) = varint(&p[hp..]);
        types.push(t);
        hp += n;
    }
    let mut dp = hlen;
    let mut vals = Vec::with_capacity(types.len());
    for t in types {
        let (v, n) = match t {
            0 => (Value::Null, 0),
            1..=6 => {
                let n = [0, 1, 2, 3, 4, 6, 8][t as usize];
                let mut x: i64 = if p[dp] & 0x80 != 0 { -1 } else { 0 };
                for i in 0..n {
                    x = (x << 8) | p[dp + i] as i64;
                }
                (Value::Int(x), n)
            }
            7 => {
                let mut a = [0u8; 8];
                a.copy_from_slice(&p[dp..dp + 8]);
                (Value::Real(f64::from_be_bytes(a)), 8)
            }
            8 => (Value::Int(0), 0),
            9 => (Value::Int(1), 0),
            t if t >= 12 && t % 2 == 0 => {
                let n = ((t - 12) / 2) as usize;
                (Value::Blob(p[dp..dp + n].to_vec()), n)
            }
            t if t >= 13 => {
                let n = ((t - 13) / 2) as usize;
                (Value::Text(String::from_utf8_lossy(&p[dp..dp + n]).into_owned()), n)
            }
            _ => return Err("SQLite: 不正なレコード".into()),
        };
        if dp + n > p.len() {
            return Err("SQLite: レコードが途中で終わっています".into());
        }
        dp += n;
        vals.push(v);
    }
    Ok(vals)
}

/// CREATE TABLE 文から列名を取り出す
fn parse_columns(sql: &str) -> Vec<String> {
    let (Some(a), Some(b)) = (sql.find('('), sql.rfind(')')) else {
        return Vec::new();
    };
    let body = &sql[a + 1..b];
    let mut cols = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in body.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch)
            }
            ')' => {
                depth -= 1;
                cur.push(ch)
            }
            ',' if depth == 0 => {
                cols.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    cols.push(cur);
    cols.into_iter()
        .filter_map(|c| {
            let c = c.trim();
            let up = c.to_ascii_uppercase();
            if up.starts_with("PRIMARY ") || up.starts_with("UNIQUE") || up.starts_with("CHECK")
                || up.starts_with("FOREIGN ") || up.starts_with("CONSTRAINT ")
            {
                return None;
            }
            let name = c.split_whitespace().next()?;
            Some(name.trim_matches(|ch| ch == '"' || ch == '`' || ch == '[' || ch == ']' || ch == '\'').to_string())
        })
        .collect()
}
