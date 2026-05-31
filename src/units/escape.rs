//! systemd unit-string escaping, ported from `simple_systemd_escape` and
//! `char2cesc` (`main.py:1010`/`:1015`). See `REFERENCE.md` §8.2.

/// C-style `\xHH` escape of every UTF-8 byte of `s`.
pub fn char2cesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 4);
    for b in s.bytes() {
        out.push_str(&format!("\\x{b:02x}"));
    }
    out
}

/// Escape a string by systemd rules. Kept as-is: `. _ : 0-9 A-Z a-z`; `/`
/// becomes `-`; everything else becomes `\xHH` over its UTF-8 bytes. When
/// `start` is true a leading `.` is escaped (systemd forbids a leading dot at
/// the start of a unit name).
pub fn simple_systemd_escape(input: &str, start: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    if start && let Some(stripped) = rest.strip_prefix('.') {
        out.push_str(&char2cesc("."));
        rest = stripped;
    }
    for ch in rest.chars() {
        if ch == '/' {
            out.push('-');
        } else if is_allowed(ch) {
            out.push(ch);
        } else {
            let mut buf = [0u8; 4];
            out.push_str(&char2cesc(ch.encode_utf8(&mut buf)));
        }
    }
    out
}

fn is_allowed(ch: char) -> bool {
    matches!(ch, '.' | '_' | ':' | '0'..='9' | 'A'..='Z' | 'a'..='z')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cesc_bytes() {
        assert_eq!(char2cesc("-"), "\\x2d");
        assert_eq!(char2cesc(" "), "\\x20");
        assert_eq!(char2cesc("."), "\\x2e");
    }

    #[test]
    fn plain_kept() {
        assert_eq!(simple_systemd_escape("sway", false), "sway");
        assert_eq!(
            simple_systemd_escape("foo.bar:baz_1", false),
            "foo.bar:baz_1"
        );
    }

    #[test]
    fn hyphen_is_escaped_not_kept() {
        assert_eq!(simple_systemd_escape("my-comp", false), "my\\x2dcomp");
    }

    #[test]
    fn slash_becomes_dash() {
        assert_eq!(simple_systemd_escape("a/b/c", false), "a-b-c");
    }

    #[test]
    fn leading_dot_guard() {
        assert_eq!(simple_systemd_escape(".hidden", true), "\\x2ehidden");
        assert_eq!(simple_systemd_escape(".hidden", false), ".hidden");
    }

    #[test]
    fn space_escaped() {
        assert_eq!(simple_systemd_escape("a b", false), "a\\x20b");
    }
}
