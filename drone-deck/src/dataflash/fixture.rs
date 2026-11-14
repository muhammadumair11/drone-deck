//! Builds DataFlash logs for tests: formats, FMTU and MULT, and rows given in the units the
//! reader returns (degrees for `L`, whole units for `c`/`e`), encoded as ArduPilot writes them.
use super::{FMT, HEAD, size};

/// A field value for [`Builder::row`].
#[derive(Clone, Copy)]
pub(crate) enum F<'a> {
    N(f64),
    S(&'a str),
}

#[derive(Default)]
pub(crate) struct Builder {
    bytes: Vec<u8>,
    types: std::collections::HashMap<u8, Vec<u8>>,
}

fn padded(text: &str, n: usize) -> Vec<u8> {
    let mut b = text.as_bytes().to_vec();
    b.resize(n, 0);
    b
}

impl Builder {
    pub(crate) fn new() -> Self {
        let mut b = Self::default();
        b.format(FMT, "FMT", "BBnNZ", "Type,Length,Name,Format,Columns");
        b.format(0x80 + 1, "MULT", "Qbd", "TimeUS,Id,Mult");
        b.format(0x80 + 2, "FMTU", "QBNN", "TimeUS,FmtType,UnitIds,MultIds");
        b
    }
    /// An FMT message for type `id`.
    pub(crate) fn format(&mut self, id: u8, name: &str, types: &str, columns: &str) -> &mut Self {
        let length = 3 + types.bytes().map(|t| size(t).unwrap()).sum::<usize>();
        self.bytes.extend(HEAD);
        self.bytes.push(FMT);
        self.bytes.push(id);
        self.bytes.push(length as u8);
        self.bytes.extend(padded(name, 4));
        self.bytes.extend(padded(types, 16));
        self.bytes.extend(padded(columns, 64));
        self.types.insert(id, types.bytes().collect());
        self
    }
    /// The multiplier ids of type `id`'s columns (FMTU).
    pub(crate) fn units(&mut self, id: u8, mults: &str) -> &mut Self {
        let units = "-".repeat(mults.len());
        self.row(
            0x80 + 2,
            &[F::N(0.), F::N(id as f64), F::S(&units), F::S(mults)],
        )
    }
    pub(crate) fn mult(&mut self, id: u8, value: f64) -> &mut Self {
        self.row(0x80 + 1, &[F::N(0.), F::N(id as f64), F::N(value)])
    }
    /// A message of type `id`.
    pub(crate) fn row(&mut self, id: u8, fields: &[F]) -> &mut Self {
        let types = self.types[&id].clone();
        assert_eq!(types.len(), fields.len(), "fields of type {id}");
        self.bytes.extend(HEAD);
        self.bytes.push(id);
        for (t, f) in types.iter().zip(fields) {
            let n = match f {
                F::N(v) => *v,
                F::S(_) => 0.,
            };
            let out: Vec<u8> = match t {
                b'b' => vec![n as i8 as u8],
                b'B' | b'M' => vec![n as u8],
                b'h' => (n as i16).to_le_bytes().to_vec(),
                b'H' => (n as u16).to_le_bytes().to_vec(),
                b'c' => ((n * 100.).round() as i16).to_le_bytes().to_vec(),
                b'C' => ((n * 100.).round() as u16).to_le_bytes().to_vec(),
                b'i' => (n as i32).to_le_bytes().to_vec(),
                b'I' => (n as u32).to_le_bytes().to_vec(),
                b'e' => ((n * 100.).round() as i32).to_le_bytes().to_vec(),
                b'E' => ((n * 100.).round() as u32).to_le_bytes().to_vec(),
                b'L' => ((n * 1e7).round() as i32).to_le_bytes().to_vec(),
                b'f' => (n as f32).to_le_bytes().to_vec(),
                b'd' => n.to_le_bytes().to_vec(),
                b'q' => (n as i64).to_le_bytes().to_vec(),
                b'Q' => (n as u64).to_le_bytes().to_vec(),
                b'a' => vec![0; 64],
                _ => match f {
                    F::S(s) => padded(s, size(*t).unwrap()),
                    F::N(_) => vec![0; size(*t).unwrap()],
                },
            };
            self.bytes.extend(out);
        }
        self
    }
    /// Raw bytes, such as damage.
    pub(crate) fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend(bytes);
        self
    }
    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}
