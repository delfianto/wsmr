//! Parse the env-preloader shell loader's output: human messages, then the
//! random mark, then a NUL-separated `env -0` dump. Ports the dump handling in
//! `prepare_env` (`main.py:2842`). See `REFERENCE.md` §3.2 step 7.

use crate::env::files::parse_env_nul;
use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Parsed result of the shell loader's stdout.
#[derive(Debug)]
pub struct ShellDump {
    /// Text printed before the mark (informational messages).
    pub messages: String,
    /// The environment captured after the mark.
    pub env: BTreeMap<String, String>,
}

/// Split `stdout` on the first occurrence of `mark`: everything before is
/// messages; the NUL-separated dump after is the environment. A missing mark
/// means the loader failed.
pub fn parse_shell_dump(stdout: &str, mark: &str) -> Result<ShellDump> {
    match stdout.find(mark) {
        None => Err(Error::Resolve(format!(
            "env output mark \"{mark}\" not found in shell output"
        ))),
        Some(pos) => {
            let messages = stdout[..pos].to_string();
            let after = stdout[pos + mark.len()..].trim_end_matches('\0');
            Ok(ShellDump {
                messages,
                env: parse_env_nul(after),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_messages_and_env() {
        let mark = "deadbeefdeadbeef";
        let stdout = format!("Loading environment from x.\n{mark}FOO=bar\0PATH=/usr/bin\0");
        let d = parse_shell_dump(&stdout, mark).unwrap();
        assert_eq!(d.messages.trim(), "Loading environment from x.");
        assert_eq!(d.env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(d.env.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn value_may_contain_equals() {
        let mark = "MARK";
        let stdout = format!("{mark}KEY=a=b=c\0");
        let d = parse_shell_dump(&stdout, mark).unwrap();
        assert_eq!(d.env.get("KEY").map(String::as_str), Some("a=b=c"));
    }

    #[test]
    fn missing_mark_errors() {
        assert!(parse_shell_dump("no mark here", "MARK").is_err());
    }

    #[test]
    fn no_messages_when_mark_first() {
        let d = parse_shell_dump("MARKFOO=1\0", "MARK").unwrap();
        assert!(d.messages.is_empty());
        assert_eq!(d.env.get("FOO").map(String::as_str), Some("1"));
    }
}
