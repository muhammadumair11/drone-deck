//! Removes personal details from text that goes into a diagnostics bundle: the user's folders
//! become `%USERPROFILE%` and friends, any other `\Users\<name>` or `/home/<name>` segment
//! becomes `<user>`, the user and computer names become placeholders, and access tokens that
//! look like Hugging Face or GitHub tokens are cut, as are the values of `KEY=value` and
//! `"key": "value"` pairs whose key names a token, password, secret or API key.

pub(crate) struct Redactor {
    /// Lower-case needle → replacement, longest first.
    literals: Vec<(String, String)>,
    /// Lower-case whole words (user and computer names) → replacement.
    words: Vec<(String, String)>,
}

/// The ways a Windows path appears in text: as written, with `/`, and escaped in JSON.
fn spellings(path: &str) -> [String; 3] {
    let path = path.trim_end_matches(['\\', '/']);
    [
        path.to_string(),
        path.replace('\\', "/"),
        path.replace('\\', "\\\\"),
    ]
}

impl Redactor {
    /// `paths` are folders to replace wherever they appear (any case); `words` are names
    /// replaced only as whole words, and only when at least three characters long.
    pub(crate) fn new(paths: &[(String, &str)], words: &[(String, &str)]) -> Self {
        let mut literals: Vec<(String, String)> = paths
            .iter()
            .filter(|(p, _)| p.trim().len() > 3)
            .flat_map(|(p, with)| {
                spellings(p)
                    .into_iter()
                    .map(move |s| (s.to_ascii_lowercase(), with.to_string()))
            })
            .collect();
        literals.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        literals.dedup_by(|a, b| a.0 == b.0);
        let mut words: Vec<(String, String)> = words
            .iter()
            .filter(|(w, _)| w.trim().chars().count() >= 3)
            .map(|(w, with)| (w.trim().to_ascii_lowercase(), with.to_string()))
            .collect();
        // `alice-pc` before `alice`, so the computer name is replaced whole.
        words.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Self { literals, words }
    }
    /// From this process's environment.
    pub(crate) fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let mut words = vec![
            (var("USERNAME"), "<user>"),
            (var("COMPUTERNAME"), "<computer>"),
        ];
        if !var("USERDOMAIN").eq_ignore_ascii_case(&var("COMPUTERNAME")) {
            words.push((var("USERDOMAIN"), "<domain>"));
        }
        Self::new(
            &[
                (var("LOCALAPPDATA"), "%LOCALAPPDATA%"),
                (var("APPDATA"), "%APPDATA%"),
                (var("TEMP"), "%TEMP%"),
                (var("USERPROFILE"), "%USERPROFILE%"),
                (var("OneDrive"), "%OneDrive%"),
            ],
            &words,
        )
    }
    pub(crate) fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (needle, with) in &self.literals {
            out = replace_ci(&out, needle, with, false);
        }
        out = user_segments(&out);
        for (word, with) in &self.words {
            out = replace_ci(&out, word, with, true);
        }
        secrets(&tokens(&out))
    }
}

/// Values of `KEY=value` / `key: value` / `"key": "value"` pairs whose key names a secret
/// (`HF_TOKEN=…`, `--api-key=…`, `"password": "…"`) become `<redacted>`.
fn secrets(text: &str) -> String {
    const KEYS: [&str; 5] = ["token", "password", "passwd", "secret", "api"];
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let key_char = |b: u8| is_word(b) || b == b'-';
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let Some(key) = KEYS
            .into_iter()
            .find(|k| lower.as_bytes()[i..].starts_with(k.as_bytes()))
        else {
            i += 1;
            continue;
        };
        // The whole key around the match, e.g. `HF_TOKEN` or `api-key`.
        let mut end = i + key.len();
        while end < bytes.len() && key_char(bytes[end]) {
            end += 1;
        }
        let name = &lower[i..end];
        if key == "api" && !(name.contains("key") || name.contains("secret")) {
            i = end;
            continue;
        }
        let mut at = end;
        if at < bytes.len() && bytes[at] == b'"' {
            at += 1;
        }
        while at < bytes.len() && bytes[at] == b' ' {
            at += 1;
        }
        if at >= bytes.len() || !matches!(bytes[at], b'=' | b':') {
            i = end;
            continue;
        }
        at += 1;
        while at < bytes.len() && bytes[at] == b' ' {
            at += 1;
        }
        let quote = (at < bytes.len() && matches!(bytes[at], b'"' | b'\'')).then(|| bytes[at]);
        let start = at + usize::from(quote.is_some());
        let mut stop = start;
        while stop < bytes.len() {
            let b = bytes[stop];
            match quote {
                // An escaped character (`\"` in JSON) is part of a quoted value.
                Some(_) if b == b'\\' => {
                    stop += 2;
                    continue;
                }
                Some(q) if b == q => break,
                None if b.is_ascii_whitespace() || b"\"',;&}]".contains(&b) => break,
                _ => {}
            }
            stop += 1;
        }
        let stop = stop.min(bytes.len());
        if stop > start {
            out.push_str(&text[last..start]);
            out.push_str("<redacted>");
            last = stop;
        }
        i = stop.max(end);
    }
    out.push_str(&text[last..]);
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Replaces every ASCII-case-insensitive occurrence of `needle` (lower case). With `whole`,
/// only where it is not joined to letters, digits or `_` on either side.
fn replace_ci(text: &str, needle: &str, with: &str, whole: bool) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(found) = lower[from..].find(needle) {
        let start = from + found;
        let end = start + needle.len();
        let bounded = !whole
            || ((start == 0 || !is_word(bytes[start - 1]))
                && (end == bytes.len() || !is_word(bytes[end])));
        if bounded {
            out.push_str(&text[last..start]);
            out.push_str(with);
            last = end;
            from = end;
        } else {
            from = start + lower[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[last..]);
    out
}

fn is_separator(b: u8) -> bool {
    b == b'\\' || b == b'/'
}
fn ends_segment(b: u8) -> bool {
    is_separator(b) || b.is_ascii_whitespace() || b"\"'`,;:()[]{}<>|".contains(&b)
}

/// `…\Users\<name>…` and `…/home/<name>…` (any separator, any case, JSON-escaped too):
/// the name becomes `<user>`, except shared profile folders.
fn user_segments(text: &str) -> String {
    const KEEP: [&str; 4] = ["public", "default", "all users", "default user"];
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let marker = ["users", "home"]
            .into_iter()
            .find(|m| lower.as_bytes()[i..].starts_with(m.as_bytes()));
        let Some(marker) = marker.filter(|_| i > 0 && is_separator(bytes[i - 1])) else {
            i += 1;
            continue;
        };
        let mut start = i + marker.len();
        if start >= bytes.len() || !is_separator(bytes[start]) {
            i += 1;
            continue;
        }
        while start < bytes.len() && is_separator(bytes[start]) {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len() && !ends_segment(bytes[end]) {
            end += 1;
        }
        let name = &lower[start..end];
        if end > start && !KEEP.contains(&name) && !name.starts_with('%') {
            out.push_str(&text[last..start]);
            out.push_str("<user>");
            last = end;
        }
        i = end.max(i + 1);
    }
    out.push_str(&text[last..]);
    out
}

/// Cuts `hf_…`, `ghp_…` and `github_pat_…` tokens after their prefix.
fn tokens(text: &str) -> String {
    const PREFIXES: [&str; 3] = ["hf_", "ghp_", "github_pat_"];
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let at_start = i == 0 || !is_word(bytes[i - 1]);
        let prefix = PREFIXES
            .into_iter()
            .find(|p| at_start && bytes[i..].starts_with(p.as_bytes()));
        if let Some(prefix) = prefix {
            let start = i + prefix.len();
            let mut end = start;
            while end < bytes.len() && is_word(bytes[end]) {
                end += 1;
            }
            if end - start >= 20 {
                out.push_str(&text[last..start]);
                out.push_str("<redacted>");
                last = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}
