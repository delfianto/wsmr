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

/// Convert a path to a `file://` URL, leaving existing URLs untouched.
pub fn path2url(arg: &str) -> String {
    if arg.contains("://") {
        return arg.to_string();
    }
    let p = std::path::Path::new(arg);
    let abs = if p.is_absolute() {
        arg.to_string()
    } else {
        std::env::current_dir()
            .map(|d| d.join(arg).to_string_lossy().into_owned())
            .unwrap_or_else(|_| arg.to_string())
    };
    format!("file://{abs}")
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
        assert_eq!(expand_str("a\\\\b"), "a\\b");
        assert_eq!(expand_str("plain"), "plain");
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
