//! systemd-run unit-name generation with systemd escaping and 255-byte
//! truncation. Ports the auto-naming in `app` (`main.py:3659`). See analysis §6.

use crate::units::escape::simple_systemd_escape;

/// Build an auto unit name: `app-<desktop>-<cmd>-<hex>.scope` or
/// `app-<desktop>-<cmd>@<hex>.service`, escaped and truncated to ≤ 255 bytes.
pub fn auto_unit_name(unit_type: &str, desktop: &str, app_name: &str, hex: &str) -> String {
    // length of the fixed parts (matches uwsm's "app---DEADBEEF." accounting)
    let l_static = "app---DEADBEEF.".len() + unit_type.len();

    let mut desktop_sub = simple_systemd_escape(desktop, false);
    if l_static + desktop_sub.len() > 127 {
        desktop_sub = truncate_escaped(&desktop_sub, 127usize.saturating_sub(l_static));
    }

    let mut cmd_sub = simple_systemd_escape(app_name, false);
    let used = l_static + desktop_sub.len();
    if used + cmd_sub.len() > 255 {
        cmd_sub = truncate_escaped(&cmd_sub, 255usize.saturating_sub(used));
    }

    match unit_type {
        "scope" => format!("app-{desktop_sub}-{cmd_sub}-{hex}.scope"),
        _ => format!("app-{desktop_sub}-{cmd_sub}@{hex}.service"),
    }
}

/// Truncate an escaped string to `max` bytes without splitting a `\xHH` token.
/// Escaped strings are ASCII, so byte indexing is safe.
fn truncate_escaped(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] == b'x' {
            // \xHH token (4 bytes) — atomic
            if i + 4 > b.len() || out.len() + 4 > max {
                break;
            }
            out.push_str(&s[i..i + 4]);
            i += 4;
        } else {
            if out.len() + 1 > max {
                break;
            }
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_and_service_forms() {
        assert_eq!(
            auto_unit_name("scope", "niri", "firefox", "deadbeef"),
            "app-niri-firefox-deadbeef.scope"
        );
        assert_eq!(
            auto_unit_name("service", "niri", "firefox", "deadbeef"),
            "app-niri-firefox@deadbeef.service"
        );
    }

    #[test]
    fn special_chars_escaped() {
        // '-' escapes to \x2d, '/' to '-'
        let n = auto_unit_name("scope", "my-de", "a/b", "00000000");
        assert_eq!(n, "app-my\\x2dde-a-b-00000000.scope");
    }

    #[test]
    fn long_name_truncated_within_255() {
        let long = "x".repeat(400);
        let n = auto_unit_name("service", "desk", &long, "deadbeef");
        assert!(n.len() <= 255, "len was {}", n.len());
        assert!(n.starts_with("app-desk-x"));
        assert!(n.ends_with("@deadbeef.service"));
    }
}
