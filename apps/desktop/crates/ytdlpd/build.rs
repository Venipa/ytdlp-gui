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
    #[cfg(windows)]
    {
        let icon = root.join("assets/windows/ytdlpd.ico");
        println!("cargo:rerun-if-changed={}", icon.display());
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().expect("a utf-8 path"))
            .compile()
            .expect("cannot embed the Windows icon");
    }
    if env::var_os("CARGO_FEATURE_REAL_ENGINE").is_some() {
        python_runtime();
    }
}

/// The exe links libpython through the engine crate: make it loadable. Windows copies the Python
/// DLLs beside the exe (app directory is first in the DLL search); unix adds an rpath to the dev
/// runtime. The shipped layout (`python/` and DLLs beside the exe, `$ORIGIN/python/lib`) is the
/// packager's job.
fn python_runtime() {
    println!("cargo:rerun-if-env-changed=PYO3_CONFIG_FILE");
    let Ok(cfg) = env::var("PYO3_CONFIG_FILE") else {
        return; // pyo3 fails with its own message; nothing to do here
    };
    println!("cargo:rerun-if-changed={cfg}");
    let text = fs::read_to_string(&cfg).expect("cannot read PYO3_CONFIG_FILE");
    let get = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_string)
    };
    if cfg!(windows) {
        let exe = PathBuf::from(get("executable").expect("pyo3.cfg has no executable="));
        let home = exe.parent().expect("python.exe has a directory");
        let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets the output"));
        // OUT_DIR = target/<profile>/build/ytdlpd-<hash>/out
        let profile = out.ancestors().nth(3).expect("profile directory");
        for entry in fs::read_dir(home)
            .expect("cannot read the python home")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".dll")
                && (name.starts_with("python") || name.starts_with("vcruntime"))
            {
                let _ = fs::copy(entry.path(), profile.join(entry.file_name()));
            }
        }
    } else if let Some(lib) = get("lib_dir") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{lib}");
        // shipped layouts, relative to the exe: AppImage/deb, flat tarball, macOS .app (libpython sits in Frameworks)
        for rel in if cfg!(target_os = "macos") {
            vec!["@executable_path/../Frameworks"]
        } else {
            // $ORIGIN/../lib: the system libs the AppImage bundles (libxkbcommon-x11, ...)
            vec![
                "$ORIGIN/../lib/ytdlpd/python/lib",
                "$ORIGIN/../lib",
                "$ORIGIN/python/lib",
            ]
        } {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{rel}");
        }
        // DT_RPATH, not DT_RUNPATH: RUNPATH only serves the exe's own needs, so a bundled lib
        // (libxkbcommon-x11) could not find the libs it needs in turn (libxcb-xkb)
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");
        }
    }
}
