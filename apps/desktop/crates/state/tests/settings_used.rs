//! Every settings key must be read somewhere outside `domain/src/settings.rs` (dead settings are bugs).
//! Keys that are deliberately not wired yet are listed in `settings_unused_allowlist.txt`; remove a key
//! from that file when a screen starts using it.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `.key` (field access) or `key:` at the start of a struct field initialiser.
fn mentions(text: &str, key: &str) -> bool {
    text.match_indices(key).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + key.len()..].chars().next();
        before == Some('.') && !after.is_some_and(is_ident)
    })
}

#[test]
fn every_setting_is_used_or_allowlisted() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let skip = [
        crates.join("domain").join("src").join("settings.rs"),
        crates.join("state").join("tests"),
    ];
    let mut files = vec![];
    rust_files(crates, &mut files);
    let corpus: String = files
        .iter()
        .filter(|f| !skip.iter().any(|s| f.starts_with(s)))
        .map(|f| std::fs::read_to_string(f).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");

    let json = serde_json::to_value(domain::Settings::default()).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    let allow: BTreeSet<String> = include_str!("settings_unused_allowlist.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let unused: BTreeSet<String> = keys
        .iter()
        .filter(|k| !mentions(&corpus, k))
        .cloned()
        .collect();
    let missing: Vec<_> = unused.difference(&allow).collect();
    assert!(
        missing.is_empty(),
        "settings never read outside domain/src/settings.rs: {missing:?} (use them or allowlist them)"
    );
    let unknown: Vec<_> = allow.iter().filter(|k| !keys.contains(k)).collect();
    assert!(
        unknown.is_empty(),
        "allowlisted keys that are not settings: {unknown:?}"
    );
}
