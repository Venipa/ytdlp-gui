use std::fs;
use std::path::{Path, PathBuf};

pub struct Item {
    pub name: String,
    pub file: PathBuf,
}

pub fn folder(dir: impl AsRef<Path>, kind: &str) -> Vec<Item> {
    let dir = dir.as_ref();
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut items: Vec<Item> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|file| file.extension().is_some_and(|found| found == kind))
        .filter_map(|file| {
            let name = file.file_stem()?.to_str()?.to_owned();
            Some(Item { name, file })
        })
        .collect();

    items.sort_by(|one, other| one.name.cmp(&other.name));
    items
}

pub fn tree(dir: impl AsRef<Path>, kind: &str) -> Vec<(String, Vec<Item>)> {
    let dir = dir.as_ref();
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut folders: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();

    folders.sort();
    folders
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_owned();
            Some((name, folder(&path, kind)))
        })
        .collect()
}

pub fn embedded(items: &[Item], key: impl Fn(&Item) -> String) -> String {
    let rows: String = items
        .iter()
        .map(|item| {
            let key = key(item);
            let file = item.file.to_str().expect("a utf-8 path");
            format!("    ({key:?}, include_bytes!({file:?}).as_slice()),\n")
        })
        .collect();

    format!("&[\n{rows}]")
}
