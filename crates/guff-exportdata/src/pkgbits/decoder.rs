//! Port of `internal/pkgbits/decoder.go`.

use std::io::{self, Cursor, Read};

use dashu::float::round::mode::HalfEven;
use dashu::float::FBig;
use dashu::integer::IBig;
use dashu::rational::RBig;
use guff_constant::{
    binary_op, make_bool, make_from_bytes, make_imag, make_int64, make_string_bytes, Value,
};

use super::codes::CodeVal;
use super::reloc::{Index, RelocEnt, RelocKind};
use super::sync::SyncMarker;
use super::version::{Field, Version, FLAG_SYNC_MARKERS};

use crate::error::Error;

const FINGERPRINT_SIZE: usize = 8;

#[derive(Debug)]
pub struct PkgDecoder {
    version: Version,
    sync: bool,
    pkg_path: String,
    elem_data: Vec<u8>,
    elem_ends: Vec<u32>,
    elem_ends_ends: [u32; RelocKind::NUM_RELOC],
}

impl PkgDecoder {
    pub fn new(pkg_path: impl Into<String>, input: &[u8]) -> Result<Self, Error> {
        let pkg_path = pkg_path.into();
        let mut cursor = Cursor::new(input);

        let ver = read_u32_le(&mut cursor)?;
        let version = Version::from_raw(ver);
        if ver >= Version::NUM_VERSIONS {
            return Err(Error::Decode(format!(
                "cannot decode {pkg_path:?}, export data version {ver} is greater than maximum supported version {}",
                Version::NUM_VERSIONS - 1
            )));
        }

        let mut sync = false;
        if version.has(Field::Flags) {
            let flags = read_u32_le(&mut cursor)?;
            sync = flags & FLAG_SYNC_MARKERS != 0;
        }

        let mut elem_ends_ends = [0u32; RelocKind::NUM_RELOC];
        for slot in &mut elem_ends_ends {
            *slot = read_u32_le(&mut cursor)?;
        }

        let total = elem_ends_ends[RelocKind::NUM_RELOC - 1] as usize;
        let mut elem_ends = vec![0u32; total];
        for slot in &mut elem_ends {
            *slot = read_u32_le(&mut cursor)?;
        }

        let pos = cursor.position() as usize;
        let elem_data = input[pos..].to_vec();
        if elem_data.len() < FINGERPRINT_SIZE
            || elem_data.len() - FINGERPRINT_SIZE != elem_ends.last().copied().unwrap_or(0) as usize
        {
            return Err(Error::Decode(format!(
                "invalid elem data length for package {pkg_path}"
            )));
        }

        Ok(Self {
            version,
            sync,
            pkg_path,
            elem_data,
            elem_ends,
            elem_ends_ends,
        })
    }

    pub fn pkg_path(&self) -> &str {
        &self.pkg_path
    }

    pub fn sync_markers(&self) -> bool {
        self.sync
    }

    pub fn num_elems(&self, k: RelocKind) -> usize {
        let idx = k.0 as usize;
        let count = self.elem_ends_ends[idx] as usize;
        if idx > 0 {
            count - self.elem_ends_ends[idx - 1] as usize
        } else {
            count
        }
    }

    pub fn abs_idx(&self, k: RelocKind, idx: Index) -> usize {
        let mut abs_idx = idx.0 as usize;
        let section = k.0 as usize;
        if section > 0 {
            abs_idx += self.elem_ends_ends[section - 1] as usize;
        }
        if abs_idx >= self.elem_ends_ends[section] as usize {
            panic!("{k:?}:{idx:?} is out of bounds");
        }
        abs_idx
    }

    pub fn data_idx(&self, k: RelocKind, idx: Index) -> &[u8] {
        let abs_idx = self.abs_idx(k, idx);
        let start = if abs_idx > 0 {
            self.elem_ends[abs_idx - 1] as usize
        } else {
            0
        };
        let end = self.elem_ends[abs_idx] as usize;
        &self.elem_data[start..end]
    }

    /// The raw bytes of a `STRING` reloc.
    ///
    /// Bytes, not `&str`: export data uses Go strings both for text (paths,
    /// names) and for arbitrary binary payloads — a `big.Int`'s little-endian
    /// magnitude, and any string constant a package exports, which can hold
    /// ill-formed UTF-8 just as a source literal can.
    pub fn string_bytes_idx(&self, idx: Index) -> &[u8] {
        self.data_idx(RelocKind::STRING, idx)
    }

    /// [`string_bytes_idx`](Self::string_bytes_idx) as text, with ill-formed
    /// bytes replaced by U+FFFD. Only for the fields that really are text.
    pub fn string_idx(&self, idx: Index) -> String {
        guff_constant::decode_lossy(self.string_bytes_idx(idx))
    }

    pub fn new_decoder(&self, k: RelocKind, idx: Index, marker: SyncMarker) -> Decoder<'_> {
        let mut r = self.new_decoder_raw(k, idx);
        r.sync(marker);
        r
    }

    pub fn temp_decoder(&self, k: RelocKind, idx: Index, marker: SyncMarker) -> Decoder<'_> {
        let mut r = self.temp_decoder_raw(k, idx);
        r.sync(marker);
        r
    }

    pub fn retire_decoder(&self, _d: Decoder<'_>) {}

    pub fn new_decoder_raw(&self, k: RelocKind, idx: Index) -> Decoder<'_> {
        let mut r = Decoder {
            common: self,
            k,
            idx,
            relocs: Vec::new(),
            data: Cursor::new(self.data_idx(k, idx)),
        };
        r.sync(SyncMarker::RELOCS);
        let n = r.len();
        r.relocs = vec![
            RelocEnt {
                kind: RelocKind(0),
                idx: Index(0)
            };
            n
        ];
        for i in 0..n {
            r.sync(SyncMarker::RELOC);
            let kind = r.len() as i32;
            let idx = r.len() as i32;
            r.relocs[i].kind = RelocKind(kind);
            r.relocs[i].idx = Index(idx);
        }
        r
    }

    pub fn temp_decoder_raw(&self, k: RelocKind, idx: Index) -> Decoder<'_> {
        self.new_decoder_raw(k, idx)
    }
}

pub struct Decoder<'a> {
    pub(crate) common: &'a PkgDecoder,
    relocs: Vec<RelocEnt>,
    data: Cursor<&'a [u8]>,
    k: RelocKind,
    idx: Index,
}

impl Decoder<'_> {
    pub fn version(&self) -> Version {
        self.common.version
    }

    fn raw_uvarint(&mut self) -> u64 {
        read_uvarint(&mut self.data).expect("uvarint")
    }

    fn raw_varint(&mut self) -> i64 {
        let ux = self.raw_uvarint();
        let mut x = (ux >> 1) as i64;
        if ux & 1 != 0 {
            x = !x;
        }
        x
    }

    fn raw_reloc(&mut self, k: RelocKind, idx: usize) -> Index {
        let e = self.relocs[idx];
        assert_eq!(e.kind, k, "reloc kind mismatch");
        e.idx
    }

    pub fn sync(&mut self, want: SyncMarker) {
        if !self.common.sync {
            return;
        }
        let pos = self.data.position();
        let have = SyncMarker(self.raw_uvarint() as i32);
        let n = self.raw_uvarint() as usize;
        for _ in 0..n {
            let _ = self.raw_uvarint();
        }
        if have != want {
            panic!(
                "export data desync: package {:?}, section {:?}, index {:?}, offset {pos}: found {:?}, expected {:?}",
                self.common.pkg_path, self.k, self.idx, have, want
            );
        }
    }

    pub fn bool(&mut self) -> bool {
        self.sync(SyncMarker::BOOL);
        let mut b = [0u8; 1];
        self.data.read_exact(&mut b).expect("bool byte");
        assert!(b[0] < 2);
        b[0] != 0
    }

    pub fn int64(&mut self) -> i64 {
        self.sync(SyncMarker::INT64);
        self.raw_varint()
    }

    pub fn uint64(&mut self) -> u64 {
        self.sync(SyncMarker::UINT64);
        self.raw_uvarint()
    }

    pub fn len(&mut self) -> usize {
        let x = self.uint64();
        let v = x as usize;
        assert_eq!(x as u64, v as u64);
        v
    }

    pub fn int(&mut self) -> i32 {
        let x = self.int64();
        let v = x as i32;
        assert_eq!(x, v as i64);
        v
    }

    pub fn uint(&mut self) -> u32 {
        let x = self.uint64();
        let v = x as u32;
        assert_eq!(x, v as u64);
        v
    }

    pub fn code(&mut self, mark: SyncMarker) -> usize {
        self.sync(mark);
        self.len()
    }

    pub fn reloc(&mut self, k: RelocKind) -> Index {
        self.sync(SyncMarker::USE_RELOC);
        let idx = self.len();
        self.raw_reloc(k, idx)
    }

    /// A `STRING` field as text, with ill-formed bytes replaced by U+FFFD.
    /// Use [`string_bytes`](Self::string_bytes) for constants and binary
    /// payloads.
    pub fn string(&mut self) -> String {
        self.sync(SyncMarker::STRING);
        self.common.string_idx(self.reloc(RelocKind::STRING))
    }

    /// A `STRING` field as the bytes it actually holds.
    pub fn string_bytes(&mut self) -> Vec<u8> {
        self.sync(SyncMarker::STRING);
        self.common
            .string_bytes_idx(self.reloc(RelocKind::STRING))
            .to_vec()
    }

    pub fn value(&mut self) -> Value {
        self.sync(SyncMarker::VALUE);
        let is_complex = self.bool();
        let mut val = self.scalar();
        if is_complex {
            val = binary_op(val, guff::token::Token::ADD, make_imag(self.scalar()));
        }
        val
    }

    fn scalar(&mut self) -> Value {
        let code = self.code(SyncMarker::VAL);
        match code {
            0 => make_bool(self.bool()),
            1 => make_string_bytes(self.string_bytes()),
            2 => make_int64(self.int64()),
            3 => self.big_int(),
            4 => {
                let num = self.big_int_ibig();
                let denom_bytes = self.string_bytes();
                use dashu::integer::UBig;
                let denom = UBig::from_le_bytes(&denom_bytes);
                Value::Rat(RBig::from_parts(num, denom))
            }
            5 => self.big_float(),
            _ => make_bool(false),
        }
    }

    fn big_int_ibig(&mut self) -> IBig {
        let bytes = self.string_bytes();
        use dashu::integer::UBig;
        let mut v = IBig::from(UBig::from_le_bytes(&bytes));
        if self.bool() {
            v = -v;
        }
        v
    }

    fn big_int(&mut self) -> Value {
        let bytes = self.string_bytes();
        let mut val = make_from_bytes(&bytes);
        if self.bool() {
            val = match val {
                Value::Int(n) => Value::Int(-n),
                Value::Int64(n) => make_int64(-n),
                other => other,
            };
        }
        val
    }

    fn big_float(&mut self) -> Value {
        let text = self.string();
        let v: FBig<HalfEven, 2> = text.parse().expect("big float parse");
        Value::Float(v)
    }
}

fn read_u32_le(r: &mut impl Read) -> Result<u32, Error> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).map_err(|e| Error::Decode(e.to_string()))?;
    Ok(u32::from_le_bytes(buf))
}

fn read_uvarint(r: &mut impl Read) -> Result<u64, io::Error> {
    let mut x = 0u64;
    let mut s = 0u32;
    for i in 0..10 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        let b = b[0];
        if b < 0x80 {
            if i == 9 && b > 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "uvarint overflow",
                ));
            }
            return Ok(x | (b as u64) << s);
        }
        x |= (b as u64 & 0x7f) << s;
        s += 7;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "uvarint overflow",
    ))
}
