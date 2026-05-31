//! Environment variable-name filtering, ported from `filter_varnames`
//! (`main.py:2633`). Drops shell-internal noise vars and rejects names that
//! don't look like POSIX environment variable names. See `REFERENCE.md` §2.

use std::collections::BTreeMap;

/// Vars always dropped regardless of validity (shell internals).
const DROP_SH_VARS: &[&str] = &["_", "SHELL", "PWD", "OLDWD"];

/// Whether `name` is a valid env var name per uwsm's `Val.sh_varname`
/// (`^([A-Za-z_][A-Za-z0-9_]+|[A-Za-z][A-Za-z0-9_]*)$`): starts with a letter
/// (any length ≥ 1), or with `_` followed by ≥ 1 word char.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        // letter-led: any number of trailing word chars
        Some(c) if c.is_ascii_alphabetic() => chars.all(is_word_char),
        // underscore-led: needs at least one trailing word char
        Some('_') => {
            let mut rest = chars.peekable();
            rest.peek().is_some() && rest.all(is_word_char)
        }
        _ => false,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether a var name should be kept (not a dropped shell var, and valid).
pub fn keep_name(name: &str) -> bool {
    !DROP_SH_VARS.contains(&name) && is_valid_name(name)
}

/// Filter an iterator of names, preserving order and dropping rejects.
pub fn filter_names<'a, I>(names: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    names.into_iter().filter(|n| keep_name(n)).collect()
}

/// Filter a `KEY=VALUE` map, dropping rejected keys.
pub fn filter_map(map: BTreeMap<String, String>) -> BTreeMap<String, String> {
    map.into_iter().filter(|(k, _)| keep_name(k)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(is_valid_name("PATH"));
        assert!(is_valid_name("A")); // single letter ok
        assert!(is_valid_name("FOO_BAR2"));
        assert!(is_valid_name("_X")); // underscore + word char
        assert!(is_valid_name("xdg_foo"));
    }

    #[test]
    fn invalid_names() {
        assert!(!is_valid_name("_")); // lone underscore
        assert!(!is_valid_name("1ABC")); // digit-led
        assert!(!is_valid_name("")); // empty
        assert!(!is_valid_name("FOO-BAR")); // hyphen not a word char
        assert!(!is_valid_name("A B"));
    }

    #[test]
    fn dropped_shell_vars() {
        assert!(!keep_name("_"));
        assert!(!keep_name("SHELL"));
        assert!(!keep_name("PWD"));
        assert!(!keep_name("OLDWD"));
        // valid + not a shell var:
        assert!(keep_name("HOME"));
        assert!(keep_name("_X"));
    }

    #[test]
    fn filter_names_preserves_order_and_drops() {
        let got = filter_names(["FOO", "_", "SHELL", "1BAD", "BAR"]);
        assert_eq!(got, vec!["FOO", "BAR"]);
    }

    #[test]
    fn filter_map_drops_bad_keys() {
        let mut m = BTreeMap::new();
        m.insert("FOO".to_string(), "1".to_string());
        m.insert("SHELL".to_string(), "/bin/sh".to_string());
        m.insert("1BAD".to_string(), "x".to_string());
        let got = filter_map(m);
        assert_eq!(got.len(), 1);
        assert!(got.contains_key("FOO"));
    }
}
