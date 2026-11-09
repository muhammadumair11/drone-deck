//! Bound allocation before reading a complete line, including malformed process output.
use std::io::{self, BufRead};
pub(crate) struct BoundedLines<R> {
    reader: R,
    limit: usize,
}
impl<R: BufRead> BoundedLines<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            limit: 4096,
        }
    }
    pub fn with_limit(reader: R, limit: usize) -> Self {
        Self {
            reader,
            limit: limit.clamp(1, 16_000_000),
        }
    }
}
impl<R: BufRead> Iterator for BoundedLines<R> {
    type Item = io::Result<String>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut line = Vec::with_capacity(self.limit.min(4096));
        let mut seen = false;
        loop {
            let bytes = match self.reader.fill_buf() {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            if bytes.is_empty() {
                return seen.then(|| {
                    Ok(String::from_utf8_lossy(&line)
                        .trim_end_matches('\r')
                        .to_owned())
                });
            }
            seen = true;
            let end = bytes.iter().position(|b| *b == b'\n');
            let consumed = end.map_or(bytes.len(), |i| i + 1);
            let text_length = end.unwrap_or(bytes.len());
            let keep = text_length.min(self.limit - line.len());
            line.extend_from_slice(&bytes[..keep]);
            self.reader.consume(consumed);
            if end.is_some() {
                return Some(Ok(String::from_utf8_lossy(&line)
                    .trim_end_matches('\r')
                    .to_owned()));
            }
        }
    }
}
#[cfg(test)]
#[path = "process_output_tests.rs"]
mod tests;
