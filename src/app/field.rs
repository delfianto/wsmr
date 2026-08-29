//! Desktop-entry `Exec` handling: value unescaping, the strict tokenizer, and
//! `%`-field expansion. Ports `entry_expand_str` / `entry_tokenize_exec` /
//! `gen_entry_args` (`main.py:288`/`:324`/`:2999`). See `REFERENCE.md` §13.

use crate::error::{Error, Result};

/// Context an entry provides for field expansion.
pub struct EntryCtx<'a> {
    /// Localized `Name` (for `%c`).
    pub name: &'a str,
    /// `Icon` (for `%i`).
    pub icon: &'a str,
    /// Entry file path (for `%k`).
    pub filename: &'a str,
}

/// Result of [`gen_entry_args`]: a single argv, or one argv per file/url when a
/// single-valued field (`%f`/`%u`) is given multiple arguments.
#[derive(Debug, PartialEq, Eq)]
pub enum GenArgs {
    /// One command line (command at `[0]`).
    Single(Vec<String>),
    /// Multiple command lines (command at `[0]` of each).
    Multi(Vec<Vec<String>>),
}

/// Unescape desktop-entry value escapes: `\s \n \t \r \\`.
pub fn expand_str(value: &str) -> String {
    if !value.contains('\\') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            out.push(match ch {
                's' => ' ',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Tokenize an (already expanded) `Exec` string per the spec quoting rules.
/// Rejects unquoted reserved characters and unescaped `` ` ``/`$` inside quotes.
pub fn tokenize_exec(value: &str) -> Result<Vec<String>> {
    let value = value.trim();
    let mut cmd: Vec<String> = Vec::new();
    let mut arg = String::new();
    let mut quoted = false;
    let mut in_space = false;
    let mut escaped = false;

    let chars = value.chars().map(Some).chain(std::iter::once(None));
    for c in chars {
        let Some(ch) = c else {
            cmd.push(std::mem::take(&mut arg));
            break;
        };
        if in_space && ch.is_whitespace() {
            continue;
        }
        in_space = false;
        if quoted {
            if escaped {
                arg.push(ch);
                escaped = false;
            } else if ch == '"' {
                quoted = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '`' || ch == '$' {
                return Err(Error::InvalidArg(format!("unescaped {ch:?} in Exec")));
            } else {
                arg.push(ch);
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
        } else if ch.is_whitespace() {
            in_space = true;
            cmd.push(std::mem::take(&mut arg));
        } else if "\t\n'\\><~|&;$*?#()`".contains(ch) {
            return Err(Error::InvalidArg(format!("unquoted {ch:?} in Exec")));
        } else {
            arg.push(ch);
        }
    }
    Ok(cmd)
}

/// Convert a path to a `file://` URL, leaving an already-schemed URL (e.g.
/// `https://…`) untouched. Ports `path2url` (`main.py:2945`) exactly,
/// including what it deliberately does *not* do: a relative path is neither
/// rejected nor resolved against the current directory — it's percent-encoded
/// and prefixed as-is (`file://relative/path`), same as upstream. Callers
/// that need an absolute URL must resolve the path before calling this.
pub fn path2url(arg: &str) -> String {
    if has_uri_scheme(arg) {
        return arg.to_string();
    }
    format!("file://{}", percent_encode(arg))
}

/// Whether `s` starts with a URI scheme (`ALPHA *(ALPHA / DIGIT / "+" / "-" /
/// ".") ":"`, RFC 3986 §3.1), matching the truthiness of Python's
/// `urllib.parse.urlparse(s).scheme`. Deliberately permissive like upstream:
/// e.g. a relative path component such as `"a:b"` is (mis)classified as
/// scheme `"a"` in both implementations — replicated for compatibility, not
/// "fixed", since callers pass whatever a desktop entry's `Exec` produced.
fn has_uri_scheme(s: &str) -> bool {
    let mut chars = s.char_indices();
    let Some((_, first)) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    for (_, c) in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
            return false;
        }
    }
    false
}

/// Percent-encode like Python's `urllib.parse.quote` with its default
/// `safe="/"`: every byte outside `A-Za-z0-9_.~-` and `/` becomes `%XX`
/// (uppercase hex), operating on `arg`'s UTF-8 bytes (so non-ASCII
/// characters, not just spaces/reserved punctuation, are correctly encoded).
fn percent_encode(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len());
    for b in arg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'~' | b'-' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Expand the `Exec` argv + caller arguments into final command line(s).
pub fn gen_entry_args(exec: &[String], args: &[String], ctx: &EntryCtx) -> Result<GenArgs> {
    let cmd = exec
        .first()
        .cloned()
        .ok_or_else(|| Error::InvalidArg("empty Exec".into()))?;

    let mut out: Vec<String> = Vec::new();
    let mut encountered_fu: Option<String> = None;
    let mut fu_idx: Option<usize> = None;

    for tok in &exec[1..] {
        if count_fields(tok) > 1 {
            return Err(Error::InvalidArg(format!(
                "more than one % field in argument: {tok:?}"
            )));
        }
        if tok.contains("%%") {
            out.push(tok.replace("%%", "%"));
        } else if has_deprecated(tok) {
            // dropped
        } else if tok.contains("%f") {
            guard_conflict(&encountered_fu, tok)?;
            encountered_fu = Some(tok.clone());
            fu_idx = Some(out.len());
            match args {
                [] => {}
                [a] => out.push(tok.replace("%f", a)),
                _ => out.push(tok.clone()), // leave for iterative replacement
            }
        } else if tok == "%F" {
            guard_conflict(&encountered_fu, tok)?;
            encountered_fu = Some(tok.clone());
            out.extend(args.iter().cloned());
        } else if tok.contains("%F") {
            return Err(Error::InvalidArg(format!(
                "\"%F\" inside argument: {tok:?}"
            )));
        } else if tok.contains("%u") {
            guard_conflict(&encountered_fu, tok)?;
            encountered_fu = Some(tok.clone());
            fu_idx = Some(out.len());
            match args {
                [] => {}
                [a] => out.push(tok.replace("%u", &path2url(a))),
                _ => out.push(tok.clone()),
            }
        } else if tok == "%U" {
            guard_conflict(&encountered_fu, tok)?;
            encountered_fu = Some(tok.clone());
            out.extend(args.iter().map(|a| path2url(a)));
        } else if tok.contains("%U") {
            return Err(Error::InvalidArg(format!(
                "\"%U\" inside argument: {tok:?}"
            )));
        } else if tok == "%c" {
            out.push(ctx.name.to_string());
        } else if tok == "%k" {
            out.push(ctx.filename.to_string());
        } else if tok == "%i" {
            if !ctx.icon.is_empty() {
                out.push("--icon".into());
                out.push(ctx.icon.to_string());
            }
        } else {
            out.push(tok.clone());
        }
    }

    if !args.is_empty() && encountered_fu.is_none() {
        return Err(Error::InvalidArg("entry does not support arguments".into()));
    }

    // multi-instance: a standalone single-valued field with >1 args
    if args.len() > 1
        && let (Some(fu), Some(idx)) = (encountered_fu.as_deref(), fu_idx)
        && (fu == "%f" || fu == "%u")
    {
        let mut instances = Vec::with_capacity(args.len());
        for a in args {
            let mut inst = out.clone();
            let repl = if fu == "%u" { path2url(a) } else { a.clone() };
            inst[idx] = inst[idx].replace(fu, &repl);
            let mut full = vec![cmd.clone()];
            full.extend(inst);
            instances.push(full);
        }
        return Ok(GenArgs::Multi(instances));
    }

    let mut full = vec![cmd];
    full.extend(out);
    Ok(GenArgs::Single(full))
}

fn guard_conflict(prev: &Option<String>, tok: &str) -> Result<()> {
    match prev {
        Some(p) => Err(Error::InvalidArg(format!(
            "conflicting Exec field args: {p:?}, {tok:?}"
        ))),
        None => Ok(()),
    }
}

/// Count `%<letter>` fields, treating `%%` as an escape.
fn count_fields(s: &str) -> usize {
    let b = s.as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() {
            if b[i + 1] == b'%' {
                i += 2;
                continue;
            }
            if b[i + 1].is_ascii_alphabetic() {
                n += 1;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    n
}

fn has_deprecated(s: &str) -> bool {
    ["%d", "%D", "%n", "%N", "%v", "%m"]
        .iter()
        .any(|f| s.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EntryCtx<'static> {
        EntryCtx {
            name: "My App",
            icon: "myicon",
            filename: "/x/my.desktop",
        }
    }

    #[test]
    fn expand_escapes() {
        assert_eq!(expand_str("a\\sb"), "a b");
        assert_eq!(expand_str("a\\nb"), "a\nb");
        assert_eq!(expand_str("a\\tb"), "a\tb");
        assert_eq!(expand_str("a\\rb"), "a\rb");
        assert_eq!(expand_str("a\\\\b"), "a\\b");
        assert_eq!(expand_str("plain"), "plain");
        // An unknown escape drops the backslash and keeps the char, matching
        // upstream's `.get(char, char)` fallback exactly (not an error, and
        // not a literal `\x` passthrough).
        assert_eq!(expand_str("a\\xb"), "axb");
        // a trailing lone backslash has nothing to escape — dropped silently
        assert_eq!(expand_str("a\\"), "a");
    }

    #[test]
    fn tokenize_basic_and_quotes() {
        assert_eq!(tokenize_exec("firefox %u").unwrap(), vec!["firefox", "%u"]);
        assert_eq!(tokenize_exec(r#""a b" c"#).unwrap(), vec!["a b", "c"]);
    }

    #[test]
    fn tokenize_rejects() {
        assert!(tokenize_exec("a;b").is_err()); // unquoted reserved
        assert!(tokenize_exec(r#""a $x""#).is_err()); // unescaped $ in quotes
    }

    /// Every reserved character from the spec's unquoted-char set,
    /// cross-checked against upstream's exact set (`main.py:386`:
    /// `"\t\n'\\><~|&;$*?#()`"`) — must be rejected unquoted, accepted quoted.
    /// `\t`/`\n` are excluded from the unquoted-reject half: both tokenizers
    /// treat them as plain argument-separating whitespace (checked *before*
    /// the reserved-char rejection), never reaching that branch at all.
    #[test]
    fn tokenize_reserved_chars_table() {
        for ch in "'\\><~|&;$*?#()`".chars() {
            let unquoted = format!("cmd a{ch}b");
            assert!(
                tokenize_exec(&unquoted).is_err(),
                "expected {ch:?} to be rejected unquoted"
            );
        }
        // \t and \n are whitespace: split the argument, not rejected
        assert_eq!(tokenize_exec("cmd a\tb").unwrap(), vec!["cmd", "a", "b"]);
        assert_eq!(tokenize_exec("cmd a\nb").unwrap(), vec!["cmd", "a", "b"]);
        // quoted, only backtick and $ are still rejected (need escaping)
        for ch in "\t\n'><~|&;*?#()".chars() {
            let quoted = format!(r#"cmd "a{ch}b""#);
            assert!(
                tokenize_exec(&quoted).is_ok(),
                "expected {ch:?} to be accepted quoted"
            );
        }
        assert!(tokenize_exec(r#"cmd "a`b""#).is_err());
        assert!(tokenize_exec(r#"cmd "a\`b""#).is_ok());
    }

    #[test]
    fn fields_single() {
        let e = vec!["app".into(), "%f".into()];
        assert_eq!(
            gen_entry_args(&e, &["/a".into()], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into(), "/a".into()])
        );
        assert_eq!(
            gen_entry_args(&e, &[], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into()])
        );
    }

    #[test]
    fn fields_multi_instance() {
        let e = vec!["app".into(), "%f".into()];
        let got = gen_entry_args(&e, &["/a".into(), "/b".into()], &ctx()).unwrap();
        assert_eq!(
            got,
            GenArgs::Multi(vec![
                vec!["app".into(), "/a".into()],
                vec!["app".into(), "/b".into()],
            ])
        );
    }

    #[test]
    fn fields_list_and_meta() {
        // %F packs all args
        let e = vec!["app".into(), "%F".into()];
        assert_eq!(
            gen_entry_args(&e, &["/a".into(), "/b".into()], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into(), "/a".into(), "/b".into()])
        );
        // %c, %i, %k, %%
        let e = vec![
            "app".into(),
            "%c".into(),
            "%i".into(),
            "%k".into(),
            "100%%".into(),
        ];
        assert_eq!(
            gen_entry_args(&e, &[], &ctx()).unwrap(),
            GenArgs::Single(vec![
                "app".into(),
                "My App".into(),
                "--icon".into(),
                "myicon".into(),
                "/x/my.desktop".into(),
                "100%".into(),
            ])
        );
    }

    #[test]
    fn args_without_field_is_error() {
        let e = vec!["app".into()];
        assert!(gen_entry_args(&e, &["/a".into()], &ctx()).is_err());
    }

    #[test]
    fn url_conversion() {
        assert_eq!(path2url("https://x/y"), "https://x/y");
        assert_eq!(path2url("/a/b"), "file:///a/b");
    }

    /// Table-tested against Python's actual
    /// `f"file://{urllib.parse.quote(arg)}"` / `urlparse(arg).scheme` output —
    /// every row here was cross-checked against a real `python3` invocation,
    /// not derived from the Rust implementation.
    #[test]
    fn path2url_table() {
        let cases: &[(&str, &str)] = &[
            ("/a b/c", "file:///a%20b/c"),                               // space
            ("/a#b", "file:///a%23b"),                                   // reserved '#'
            ("/a%b", "file:///a%25b"),                                   // literal '%' re-encoded
            ("/h\u{e9}llo/w\u{f6}rld", "file:///h%C3%A9llo/w%C3%B6rld"), // Unicode (Latin-1 supplement)
            (
                "/\u{65e5}\u{672c}\u{8a9e}",
                "file:///%E6%97%A5%E6%9C%AC%E8%AA%9E",
            ), // Unicode (CJK, multi-byte UTF-8)
            ("https://x/y", "https://x/y"),                              // already a URL, untouched
            ("mailto:a@b.com", "mailto:a@b.com"),                        // scheme without `//`
            ("a:b", "a:b"), // (mis)classified as scheme "a" — matches upstream
            ("/a:b", "file:///a%3Ab"), // leading '/' disqualifies scheme detection
            ("relative/path", "file://relative/path"), // no cwd resolution, matches upstream
            ("", "file://"), // malformed/empty input
        ];
        for (input, expected) in cases {
            assert_eq!(path2url(input), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn url_fields_single_and_multi() {
        // %u single arg → converted to URL
        let e = vec!["app".into(), "%u".into()];
        assert_eq!(
            gen_entry_args(&e, &["/a".into()], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into(), "file:///a".into()])
        );
        // %u multi → one instance per arg
        assert_eq!(
            gen_entry_args(&e, &["/a".into(), "/b".into()], &ctx()).unwrap(),
            GenArgs::Multi(vec![
                vec!["app".into(), "file:///a".into()],
                vec!["app".into(), "file:///b".into()],
            ])
        );
        // %U packs all as URLs
        let e = vec!["app".into(), "%U".into()];
        assert_eq!(
            gen_entry_args(&e, &["https://x".into(), "/b".into()], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into(), "https://x".into(), "file:///b".into()])
        );
    }

    #[test]
    fn deprecated_fields_dropped_and_no_args() {
        // %d is deprecated → dropped; with no caller args that's fine
        let e = vec!["app".into(), "%d".into(), "tail".into()];
        assert_eq!(
            gen_entry_args(&e, &[], &ctx()).unwrap(),
            GenArgs::Single(vec!["app".into(), "tail".into()])
        );
    }

    #[test]
    fn field_errors() {
        // more than one % field in a single token
        assert!(gen_entry_args(&["app".into(), "%f%u".into()], &[], &ctx()).is_err());
        // %F embedded in an argument (not standalone)
        assert!(gen_entry_args(&["app".into(), "x%F".into()], &[], &ctx()).is_err());
        // %U embedded in an argument
        assert!(gen_entry_args(&["app".into(), "x%U".into()], &[], &ctx()).is_err());
        // conflicting file/url fields
        assert!(
            gen_entry_args(
                &["app".into(), "%f".into(), "%u".into()],
                &["/a".into()],
                &ctx()
            )
            .is_err()
        );
        // empty Exec
        assert!(gen_entry_args(&[], &[], &ctx()).is_err());
    }

    #[test]
    fn icon_omitted_when_empty() {
        let c = EntryCtx {
            name: "n",
            icon: "",
            filename: "/x",
        };
        assert_eq!(
            gen_entry_args(&["app".into(), "%i".into()], &[], &c).unwrap(),
            GenArgs::Single(vec!["app".into()])
        );
    }
}
