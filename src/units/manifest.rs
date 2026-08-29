//! Ownership manifest for wsmr-generated files in a rung directory.
//!
//! Generation and removal must never treat a path as wsmr's own just because
//! its name matches a pattern wsmr recognizes — a foreign unit (hand-written,
//! or written by uwsm, which currently shares this unit namespace) could
//! occupy the exact same path. The manifest
//! records, for every relative path wsmr has written, a content fingerprint
//! of what wsmr put there; a path counts as wsmr-owned only when both the
//! manifest lists it *and* the file on disk still carries that exact
//! fingerprint. Anything else — absent from the manifest, or present but
//! drifted since wsmr last wrote it — is treated as foreign and is never
//! overwritten or deleted.

use crate::error::{Error, Result};
use crate::util::fsutil;
use std::collections::BTreeMap;
use std::path::Path;

/// Manifest file name. Deliberately not a systemd unit suffix, so the user
/// manager ignores it when scanning the unit directory.
pub const MANIFEST_NAME: &str = ".wsmr-generation";

/// Per-rung-directory ownership manifest: relative path -> content
/// fingerprint of what wsmr last wrote there.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    entries: BTreeMap<String, u64>,
}

impl Manifest {
    /// Load the manifest for `dir`. A missing file is an empty manifest —
    /// either the first generation wsmr has ever done there, or everything
    /// it once owned has since been removed.
    pub fn load(dir: &Path) -> Result<Manifest> {
        let path = dir.join(MANIFEST_NAME);
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(Manifest::parse(&s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(Error::io(&path, e)),
        }
    }

    fn parse(s: &str) -> Manifest {
        let mut entries = BTreeMap::new();
        for line in s.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((hash, relname)) = line.split_once(' ')
                && let Ok(fp) = u64::from_str_radix(hash, 16)
                && !relname.is_empty()
            {
                entries.insert(relname.to_string(), fp);
            }
        }
        Manifest { entries }
    }

    fn render(&self) -> String {
        let mut out =
            String::from("# wsmr-generated manifest \u{2014} machine-written, do not edit\n");
        for (relname, fp) in &self.entries {
            out.push_str(&format!("{fp:016x} {relname}\n"));
        }
        out
    }

    /// Atomically persist the manifest into `dir`.
    pub fn save(&self, dir: &Path) -> Result<()> {
        fsutil::atomic_write(dir, MANIFEST_NAME, &self.render())
    }

    /// Whether `relname`'s recorded fingerprint matches `disk_content` — the
    /// path is both tracked *and* still holds exactly what wsmr last wrote.
    pub fn verify(&self, relname: &str, disk_content: &str) -> bool {
        self.entries.get(relname) == Some(&fingerprint(disk_content))
    }

    /// Record that wsmr wrote `content` at `relname`.
    pub fn record(&mut self, relname: &str, content: &str) {
        self.entries
            .insert(relname.to_string(), fingerprint(content));
    }

    /// Forget a path wsmr no longer owns (removed, or failed verification).
    pub fn forget(&mut self, relname: &str) {
        self.entries.remove(relname);
    }

    /// All currently tracked relative paths.
    pub fn tracked(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

/// Non-cryptographic content fingerprint (64-bit FNV-1a). This only needs to
/// detect drift between what wsmr wrote and what's on disk now — every
/// candidate path is already writable by the local user, so cryptographic
/// collision-resistance buys nothing here.
fn fingerprint(content: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = std::env::temp_dir().join(format!(
                "wsmr-manifest-{}-{}",
                std::process::id(),
                nanos
            ));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_manifest_is_empty() {
        let td = TempDir::new();
        let m = Manifest::load(td.path()).unwrap();
        assert!(!m.verify("a.service", "anything"));
        assert!(m.tracked().next().is_none());
    }

    #[test]
    fn record_then_verify_round_trips_through_disk() {
        let td = TempDir::new();
        let mut m = Manifest::default();
        m.record("wayland-wm@.service", "content-v1\n");
        m.save(td.path()).unwrap();

        let loaded = Manifest::load(td.path()).unwrap();
        assert!(loaded.verify("wayland-wm@.service", "content-v1\n"));
        assert!(!loaded.verify("wayland-wm@.service", "content-v2\n"));
        assert!(!loaded.verify("other.service", "content-v1\n"));
    }

    #[test]
    fn forget_drops_ownership() {
        let mut m = Manifest::default();
        m.record("x.service", "body\n");
        assert!(m.verify("x.service", "body\n"));
        m.forget("x.service");
        assert!(!m.verify("x.service", "body\n"));
    }

    #[test]
    fn a_tampered_manifest_line_is_ignored_not_trusted() {
        let td = TempDir::new();
        // Hand-craft a manifest with garbage/malformed lines mixed with a
        // valid one — parsing must not panic or authorize bogus entries.
        std::fs::write(
            td.path().join(MANIFEST_NAME),
            "not-a-valid-line\n\
             deadbeefcafef00d wayland-wm@sway.service.d/50_custom.conf\n\
             garbage-hash some/path\n",
        )
        .unwrap();
        let m = Manifest::load(td.path()).unwrap();
        assert_eq!(m.tracked().count(), 1);
        assert!(m.tracked().eq(["wayland-wm@sway.service.d/50_custom.conf"]));
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive_to_content() {
        assert_eq!(fingerprint("abc"), fingerprint("abc"));
        assert_ne!(fingerprint("abc"), fingerprint("abd"));
    }
}
