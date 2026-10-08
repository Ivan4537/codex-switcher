//! Identity for requests created by Switcher's Desktop-only features.
//! Forwarded client requests never use this module.
use std::path::{Path, PathBuf};
use std::process::Command;

fn valid_version(value: &str) -> Option<String> {
    let version = value.trim();
    (!version.is_empty()
        && version.len() <= 128
        && version.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-+_".contains(&byte)))
    .then(|| version.to_string())
}

#[cfg(target_os = "macos")]
fn bundle_version(bundle: &Path) -> Option<String> {
    let plist = bundle.join("Contents/Info.plist");
    let read = |key: &str| {
        let output = Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw"])
            .arg(&plist)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    // Ignore unrelated or renamed bundles claiming the same display name.
    if read("CFBundleIdentifier")?.as_str() != "com.openai.codex" {
        return None;
    }
    valid_version(&read("CFBundleShortVersionString")?)
}

#[cfg(target_os = "macos")]
fn installed_version() -> Option<String> {
    let mut candidates = vec![
        PathBuf::from("/Applications/ChatGPT.app"),
        PathBuf::from("/Applications/Codex.app"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let apps = PathBuf::from(home).join("Applications");
        candidates.extend([apps.join("ChatGPT.app"), apps.join("Codex.app")]);
    }
    candidates.iter().find_map(|bundle| bundle_version(bundle))
}

#[cfg(not(target_os = "macos"))]
fn installed_version() -> Option<String> {
    // Do not invent an installed version on platforms without a verified probe.
    None
}

pub(crate) fn user_agent() -> String {
    // Deliberately not cached for the process lifetime: an app update must be
    // reflected by the next request, without requiring a Switcher update.
    let version = installed_version().unwrap_or_else(|| "unknown".to_string());
    format!(
        "ChatGPT/{version} ({} {}; {})",
        crate::codex_ua::os_type(),
        crate::codex_ua::os_version(),
        crate::codex_ua::arch()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_bundle_version_but_rejects_header_injection() {
        assert_eq!(
            valid_version(" 26.930.31730 \n").as_deref(),
            Some("26.930.31730")
        );
        for invalid in ["", "unknown", "1.2.3\r\nInjected: true", "1.2 3", "1.2/3"] {
            assert!(valid_version(invalid).is_none());
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn reads_changed_bundle_version_and_never_substitutes_a_pinned_version() {
        let root = std::env::temp_dir().join(format!("desktop-ua-test-{}", uuid::Uuid::new_v4()));
        let contents = root.join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        for version in ["1.2.3", "4.5.6"] {
            std::fs::write(contents.join("Info.plist"), format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.openai.codex</string><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
            assert_eq!(bundle_version(&root).as_deref(), Some(version));
        }
        std::fs::write(contents.join("Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>other.app</string><key>CFBundleShortVersionString</key><string>9.9.9</string></dict></plist>").unwrap();
        assert_eq!(bundle_version(&root), None);
        assert_eq!(bundle_version(&root.join("missing")), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
