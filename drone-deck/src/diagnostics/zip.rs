//! A minimal ZIP writer: stored (uncompressed) entries, UTF-8 names, no ZIP64. Enough for a
//! diagnostics bundle of a few megabytes that Explorer and every unzip tool open.

/// CRC-32 (IEEE 802.3, reflected, as ZIP uses it).
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        table
    });
    !bytes.iter().fold(!0u32, |c, &b| {
        table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// MS-DOS date and time fields from a local `YYYY-MM-DD HH:MM:SS` stamp; 1980-01-01 when it
/// does not parse.
pub(crate) fn dos_time(stamp: &str) -> (u16, u16) {
    let digits: Vec<u16> = stamp
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    match digits[..] {
        [y, mo, d, h, mi, s, ..] if (1980..2108).contains(&y) => (
            ((y - 1980) << 9) | (mo << 5) | d,
            (h << 11) | (mi << 5) | (s / 2),
        ),
        _ => ((1 << 5) | 1, 0),
    }
}

/// Builds a ZIP archive holding `entries` (name, bytes), all dated `(date, time)`.
/// Names use `/` separators. Entries and the archive must stay under 4 GiB.
pub(crate) fn store(entries: &[(String, Vec<u8>)], (date, time): (u16, u16)) -> Vec<u8> {
    const UTF8_NAMES: u16 = 1 << 11;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name = name.as_bytes();
        let put16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let put32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());

        put32(&mut out, 0x0403_4b50);
        put16(&mut out, 20);
        put16(&mut out, UTF8_NAMES);
        put16(&mut out, 0);
        put16(&mut out, time);
        put16(&mut out, date);
        put32(&mut out, crc);
        put32(&mut out, size);
        put32(&mut out, size);
        put16(&mut out, name.len() as u16);
        put16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        put32(&mut central, 0x0201_4b50);
        put16(&mut central, 20);
        put16(&mut central, 20);
        put16(&mut central, UTF8_NAMES);
        put16(&mut central, 0);
        put16(&mut central, time);
        put16(&mut central, date);
        put32(&mut central, crc);
        put32(&mut central, size);
        put32(&mut central, size);
        put16(&mut central, name.len() as u16);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(name);
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);
    let count = entries.len() as u16;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}
