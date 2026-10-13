//! Portable, one-way Server → Client Skills mirrors. Local skills are never owned implicitly.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

const LIMIT: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Bundle {
    pub schema: u32,
    pub source_home: String,
    pub skills: Vec<Skill>,
    pub skipped: BTreeMap<String, String>,
}
#[derive(Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub files: BTreeMap<String, File>,
}
#[derive(Serialize, Deserialize)]
pub struct File {
    pub data: String,
    pub executable: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    source: String,
    skills: BTreeMap<String, String>,
}

/// A Server mirror must not be uploaded back after client-side path adaptation.
pub fn managed_roots(home: &Path) -> Result<Vec<String>, String> {
    let path = home.join(".codex-switcher/skills-pull-manifest.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(manifest
        .skills
        .keys()
        .filter_map(|name| name.split('/').next().map(str::to_owned))
        .collect())
}
#[derive(Default, Serialize, Deserialize)]
pub struct Report {
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub skipped: BTreeMap<String, String>,
    pub conflicts: Vec<String>,
    pub retired: Vec<String>,
}
#[derive(Default, Deserialize)]
struct Compatibility {
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
}
fn safe_relative(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\\')
        && !s.contains(':')
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn excluded(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "target" | "dist" | "__pycache__" | "venv"
        )
}
fn walk(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, File>,
    total: &mut usize,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if excluded(&name) || name.ends_with(".pyc") || name.ends_with(".log") {
            continue;
        }
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        // Never follow a link into account stores, credentials, or another project.
        if ty.is_symlink() {
            return Err("contains symbolic links; publish a self-contained skill".into());
        }
        if ty.is_dir() {
            walk(root, &p, files, total)?;
        } else if ty.is_file() {
            let size = entry.metadata().map_err(|e| e.to_string())?.len() as usize;
            if size > 2 * 1024 * 1024 || *total + size > LIMIT {
                return Err("export size limit exceeded".into());
            }
            let data = fs::read(&p).map_err(|e| e.to_string())?;
            if let Ok(text) = std::str::from_utf8(&data) {
                let secret = regex::Regex::new(r#"(?im)(-----BEGIN (?:OPENSSH |RSA |EC )?PRIVATE KEY-----|(?:password|passwd|secret|api_key|access_token)\s*[:=]\s*[\"'][^\"'\s${}<>]{4,}[\"'])"#).unwrap();
                if secret.is_match(text) {
                    return Err("contains embedded credentials; use environment credentials".into());
                }
            }
            *total += size;
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                entry
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .permissions()
                    .mode()
                    & 0o111
                    != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(
                rel,
                File {
                    data: STANDARD.encode(data),
                    executable,
                },
            );
        }
    }
    Ok(())
}
fn discover(root: &Path, current: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    if current.join("SKILL.md").is_file() {
        result.push(current.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(current).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_dir()
            && !excluded(&entry.file_name().to_string_lossy())
        {
            discover(root, &entry.path(), result)?;
        }
    }
    let _ = root;
    Ok(())
}
pub fn export(root: &Path, home: &Path) -> Result<Bundle, String> {
    let mut bundle = Bundle {
        schema: 1,
        source_home: home.to_string_lossy().into(),
        skills: vec![],
        skipped: BTreeMap::new(),
    };
    let mut roots = vec![];
    discover(root, root, &mut roots)?;
    roots.sort();
    let mut total = 0;
    for p in roots {
        let name = p
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let mut files = BTreeMap::new();
        match walk(&p, &p, &mut files, &mut total) {
            Ok(()) => bundle.skills.push(Skill { name, files }),
            Err(e) => {
                bundle.skipped.insert(name, e);
            }
        }
    }
    Ok(bundle)
}
fn executable_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
fn adapt(
    files: &BTreeMap<String, File>,
    source_home: &str,
    home: &Path,
    platform: &str,
) -> Result<BTreeMap<String, File>, String> {
    let mut result = BTreeMap::new();
    let home = home.to_string_lossy();
    if let Some(meta) = files.get("compatibility.json") {
        let meta: Compatibility =
            serde_json::from_slice(&STANDARD.decode(&meta.data).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if !meta.platforms.is_empty() && !meta.platforms.iter().any(|p| p == platform) {
            return Err(format!("unsupported platform: {platform}"));
        }
        for dep in meta.requires {
            if executable_path(&dep).is_none() {
                return Err(format!("missing command: {dep}"));
            }
        }
    }
    let mac = regex::Regex::new(r"(?i)\b(?:osascript|launchctl|pbcopy|pbpaste)\b|/Applications/|security\s+(?:find|add|delete)-|macos\s+keychain").unwrap();
    let brew = regex::Regex::new(r"/opt/homebrew/bin/([A-Za-z0-9_.-]+)").unwrap();
    for (name, file) in files {
        if !safe_relative(name) {
            return Err("invalid file path".into());
        }
        let mut data = STANDARD.decode(&file.data).map_err(|e| e.to_string())?;
        if let Ok(text) = std::str::from_utf8(&data) {
            if platform != "macos" && mac.is_match(text) {
                return Err("requires macOS commands or applications".into());
            }
            let mut text = text.replace(source_home, &home);
            if platform == "linux" {
                for capture in brew.captures_iter(&text.clone()) {
                    let command = &capture[1];
                    let path = executable_path(command)
                        .ok_or_else(|| format!("missing command: {command}"))?;
                    text = text.replace(&capture[0], &path.to_string_lossy());
                }
                if text.contains("/opt/homebrew/") || text.contains("/Library/Application Support/")
                {
                    return Err("requires macOS installation paths".into());
                }
                if text.contains("/Users/") {
                    return Err("contains another machine's macOS path".into());
                }
            }
            data = text.into_bytes();
        }
        result.insert(
            name.clone(),
            File {
                data: STANDARD.encode(data),
                executable: file.executable,
            },
        );
    }
    Ok(result)
}
fn hash(files: &BTreeMap<String, File>) -> String {
    let mut h = Sha256::new();
    for (p, f) in files {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
        h.update([u8::from(f.executable)]);
        h.update(&f.data);
    }
    format!("{:x}", h.finalize())
}
fn disk_hash(p: &Path) -> Result<String, String> {
    let mut files = BTreeMap::new();
    walk(p, p, &mut files, &mut 0)?;
    Ok(hash(&files))
}
/// Network failures happen before this function: they never remove a local mirror.
pub fn apply(
    bundle: Bundle,
    root: &Path,
    home: &Path,
    platform: &str,
    source: &str,
    blacklist: &[String],
) -> Result<Report, String> {
    let windows_home = regex::Regex::new(r"^[A-Za-z]:[\\/].+")
        .unwrap()
        .is_match(&bundle.source_home);
    if bundle.schema != 1
        || bundle.source_home.len() < 3
        || (!Path::new(&bundle.source_home).is_absolute() && !windows_home)
    {
        return Err("unsupported Skills bundle".into());
    }
    let state = home.join(".codex-switcher");
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    let manifest_path = state.join("skills-pull-manifest.json");
    let mut manifest: Manifest = if manifest_path.exists() {
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        Manifest::default()
    };
    if !manifest.source.is_empty() && manifest.source != source {
        return Err("Skills source changed; review the existing mirror manifest first".into());
    }
    for name in manifest.skills.keys() {
        if !safe_relative(name) || name.split('/').any(|c| c.starts_with('.')) {
            return Err("invalid managed skill name".into());
        }
    }
    let mut report = Report {
        skipped: bundle.skipped,
        ..Default::default()
    };
    let mut prepared = BTreeMap::new();
    let mut total = 0;
    for skill in bundle.skills {
        if !safe_relative(&skill.name) || skill.name.split('/').any(|c| c.starts_with('.')) {
            return Err("invalid skill name".into());
        }
        if prepared.contains_key(&skill.name) {
            return Err("duplicate skill name".into());
        }
        if !skill.files.contains_key("SKILL.md") {
            return Err("skill is missing SKILL.md".into());
        }
        if blacklist.iter().any(|s| s == &skill.name) {
            report
                .skipped
                .insert(skill.name, "client exclusion list".into());
            continue;
        }
        match adapt(&skill.files, &bundle.source_home, home, platform) {
            Ok(files) => {
                for file in files.values() {
                    total += file.data.len();
                }
                if total > LIMIT * 2 {
                    return Err("Skills bundle too large".into());
                }
                prepared.insert(skill.name, files);
            }
            Err(e) => {
                report.skipped.insert(skill.name, e);
            }
        }
    }
    // Do not invent project paths. Also exclude dependants of skills filtered above.
    let paths = regex::Regex::new(&format!(
        r#"{}[^\s\"'`<>\)\],;]+"#,
        regex::escape(&home.to_string_lossy())
    ))
    .unwrap();
    loop {
        let mut rejected = vec![];
        for (name, files) in &prepared {
            'files: for file in files.values() {
                let bytes = STANDARD.decode(&file.data).map_err(|e| e.to_string())?;
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    for matched in paths.find_iter(text) {
                        let p = Path::new(matched.as_str().trim_end_matches('.'));
                        let aliases = [
                            home.join(".codex-switcher/skills"),
                            home.join(".codex/skills"),
                            home.join(".claude/skills"),
                        ];
                        let skill_rel = aliases.iter().find_map(|a| p.strip_prefix(a).ok());
                        let exists = if let Some(rel) = skill_rel {
                            prepared.iter().any(|(n, fs)| {
                                rel.strip_prefix(n).ok().is_some_and(|f| {
                                    f.as_os_str().is_empty()
                                        || fs.contains_key(&f.to_string_lossy().replace('\\', "/"))
                                })
                            })
                        } else {
                            p.exists()
                        };
                        if !exists
                            && (skill_rel.is_some()
                                || matched.as_str().contains("/Documents/projects/"))
                        {
                            rejected.push((
                                name.clone(),
                                format!("missing local dependency: {}", p.display()),
                            ));
                            break 'files;
                        }
                    }
                }
            }
        }
        if rejected.is_empty() {
            break;
        }
        for (name, reason) in rejected {
            prepared.remove(&name);
            report.skipped.insert(name, reason);
        }
    }
    let backup = state
        .join("skills-pull-backups")
        .join(uuid::Uuid::new_v4().to_string());
    for (name, files) in prepared {
        let target = root.join(&name);
        // Refuse any symlink parent supplied locally, including nested containers.
        let mut parent = root.to_path_buf();
        for c in Path::new(&name).components() {
            parent.push(c);
            if parent.is_symlink() {
                return Err("local mirror path contains a symbolic link".into());
            }
        }
        let digest = hash(&files);
        if target.exists() {
            let actual = disk_hash(&target)?;
            if manifest.skills.get(&name) != Some(&actual) {
                report.conflicts.push(name);
                continue;
            }
            if actual == digest {
                report.unchanged.push(name);
                continue;
            }
        }
        let staging = state.join(format!("skills-staging-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        for (path, file) in &files {
            let p = staging.join(path);
            fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(&p, STANDARD.decode(&file.data).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &p,
                    fs::Permissions::from_mode(if file.executable { 0o755 } else { 0o644 }),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        let previous = backup.join(&name);
        if target.exists() {
            fs::create_dir_all(previous.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::rename(&target, &previous).map_err(|e| e.to_string())?;
        }
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&staging, &target) {
            if previous.exists() {
                let _ = fs::rename(&previous, &target);
            }
            return Err(e.to_string());
        }
        manifest.skills.insert(name.clone(), digest);
        report.updated.push(name);
    }
    // Only previously managed, unmodified skills may be retired (into a backup).
    for name in manifest.skills.keys().cloned().collect::<Vec<_>>() {
        if report.updated.contains(&name)
            || report.unchanged.contains(&name)
            || report.conflicts.contains(&name)
        {
            continue;
        }
        let target = root.join(&name);
        if target.exists() && disk_hash(&target)? == manifest.skills[&name] {
            let dest = backup.join(&name);
            fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::rename(&target, dest).map_err(|e| e.to_string())?;
            report.retired.push(name.clone());
            manifest.skills.remove(&name);
        } else if target.exists() {
            report.conflicts.push(name);
        } else {
            manifest.skills.remove(&name);
        }
    }
    manifest.source = source.into();
    let temp = state.join("skills-pull-manifest.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temp, manifest_path).map_err(|e| e.to_string())?;
    fs::write(
        state.join("skills-pull-report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("skills-pull-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn root(&self) -> PathBuf {
            self.0.join(".codex-switcher/skills")
        }
        fn bundle(&self, content: &str) -> Bundle {
            Bundle {
                schema: 1,
                source_home: "/Users/author".into(),
                skills: vec![Skill {
                    name: "shared".into(),
                    files: BTreeMap::from([(
                        "SKILL.md".into(),
                        File {
                            data: STANDARD.encode(content),
                            executable: false,
                        },
                    )]),
                }],
                skipped: BTreeMap::new(),
            }
        }
        fn apply(&self, b: Bundle) -> Result<Report, String> {
            apply(b, &self.root(), &self.0, "linux", "server", &[])
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn arbitrary_user_home_and_incremental_conflicts() {
        let f = Fixture::new();
        let b = || f.bundle("Use /Users/author/.codex-switcher/skills/shared/SKILL.md");
        assert_eq!(f.apply(b()).unwrap().updated, ["shared"]);
        assert!(fs::read_to_string(f.root().join("shared/SKILL.md"))
            .unwrap()
            .contains(&f.0.to_string_lossy().to_string()));
        assert_eq!(f.apply(b()).unwrap().unchanged, ["shared"]);
        fs::write(f.root().join("shared/SKILL.md"), "local edit").unwrap();
        assert_eq!(f.apply(b()).unwrap().conflicts, ["shared"]);
        assert_eq!(
            fs::read_to_string(f.root().join("shared/SKILL.md")).unwrap(),
            "local edit"
        );
    }
    #[test]
    fn mac_and_missing_project_excluded() {
        let f = Fixture::new();
        assert!(f
            .apply(f.bundle("Run osascript"))
            .unwrap()
            .skipped
            .contains_key("shared"));
        assert!(f
            .apply(f.bundle("Run /Users/author/Documents/projects/absent/main.py"))
            .unwrap()
            .skipped
            .contains_key("shared"));
        assert!(!f.root().join("shared").exists());
    }
    #[test]
    fn custom_local_and_builtin_skills_preserved() {
        let f = Fixture::new();
        fs::create_dir_all(f.root().join("shared")).unwrap();
        fs::write(f.root().join("shared/SKILL.md"), "local").unwrap();
        fs::create_dir_all(f.root().join(".system")).unwrap();
        fs::write(f.root().join(".system/keep"), "builtin").unwrap();
        assert_eq!(f.apply(f.bundle("remote")).unwrap().conflicts, ["shared"]);
        assert!(f.root().join(".system/keep").exists());
    }
    #[test]
    fn path_traversal_and_file_traversal_rejected() {
        let f = Fixture::new();
        let mut b = f.bundle("test");
        b.skills[0].name = "../escape".into();
        assert!(f.apply(b).is_err());
        let mut b = f.bundle("test");
        b.skills[0].files.insert(
            "../escape".into(),
            File {
                data: STANDARD.encode("bad"),
                executable: false,
            },
        );
        assert!(f.apply(b).unwrap().skipped.contains_key("shared"));
        assert!(!f.0.join("escape").exists());
    }
    #[test]
    fn managed_removal_archived_and_source_change_refused() {
        let f = Fixture::new();
        f.apply(f.bundle("before")).unwrap();
        let mut b = f.bundle("test");
        b.skills.clear();
        assert_eq!(f.apply(b).unwrap().retired, ["shared"]);
        assert!(!f.root().join("shared").exists());
        assert!(f.0.join(".codex-switcher/skills-pull-backups").exists());
        assert!(apply(
            f.bundle("next"),
            &f.root(),
            &f.0,
            "linux",
            "other-server",
            &[]
        )
        .is_err());
    }
    #[test]
    fn platform_metadata_and_transitive_dependencies() {
        let f = Fixture::new();
        let mut b = f.bundle("portable");
        b.skills[0].files.insert(
            "compatibility.json".into(),
            File {
                data: STANDARD.encode(r#"{"platforms":["macos"]}"#),
                executable: false,
            },
        );
        b.skills.push(Skill {
            name: "depends".into(),
            files: BTreeMap::from([(
                "SKILL.md".into(),
                File {
                    data: STANDARD.encode("Use /Users/author/.codex/skills/shared/SKILL.md"),
                    executable: false,
                },
            )]),
        });
        let r = f.apply(b).unwrap();
        assert_eq!(r.skipped.len(), 2);
        assert!(r.updated.is_empty());
    }
    #[test]
    fn exports_self_contained_skills_without_credentials() {
        let f = Fixture::new();
        fs::create_dir_all(f.root().join("safe")).unwrap();
        fs::write(f.root().join("safe/SKILL.md"), "hello").unwrap();
        fs::create_dir_all(f.root().join("secret")).unwrap();
        fs::write(f.root().join("secret/SKILL.md"), "password = 'test-value'").unwrap();
        let b = export(&f.root(), &f.0).unwrap();
        assert_eq!(b.skills.len(), 1);
        assert!(b.skipped.contains_key("secret"));
    }
}
