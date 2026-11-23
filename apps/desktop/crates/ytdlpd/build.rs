use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo names the crate"));
    let root = manifest
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels under the workspace");
    let faces = embed::folder(root.join("assets/fonts"), "ttf");
    let embedded = embed::embedded(&faces, |item| format!("fonts/{}.ttf", item.name));
    let source = format!("const FONTS: &[(&str, &[u8])] = {embedded};\n");
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets the output")).join("fonts.rs");
    fs::write(&out, source).expect("cannot write the font registry");
    println!("cargo:rerun-if-changed=build.rs");
}
