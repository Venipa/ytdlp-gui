//! Make the PBS libpython findable: copy the DLLs next to the built exes (Windows), add an rpath (unix),
//! and bake the dev python home so examples/tests run without a layout.
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PYO3_CONFIG_FILE");
    let Ok(cfg) = env::var("PYO3_CONFIG_FILE") else {
        return;
    };
    println!("cargo:rerun-if-changed={cfg}");
    let text = fs::read_to_string(&cfg).expect("read PYO3_CONFIG_FILE");
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string))
    };
    let exe = PathBuf::from(get("executable").expect("executable= in pyo3 cfg"));
    let home = if cfg!(windows) {
        exe.parent().unwrap().to_path_buf()
    } else {
        exe.parent().unwrap().parent().unwrap().to_path_buf()
    };
    println!("cargo:rustc-env=YTDLPD_DEV_PYTHON_HOME={}", home.display());

    if cfg!(windows) {
        // OUT_DIR = target/<profile>/build/engine-<hash>/out
        let out = PathBuf::from(env::var("OUT_DIR").unwrap());
        let profile = out.ancestors().nth(3).expect("profile dir").to_path_buf();
        for entry in fs::read_dir(&home).unwrap().flatten() {
            let n = entry.file_name().to_string_lossy().to_lowercase();
            if n.ends_with(".dll") && (n.starts_with("python") || n.starts_with("vcruntime")) {
                for sub in ["", "deps", "examples"] {
                    let d = profile.join(sub);
                    if d.is_dir() {
                        let _ = fs::copy(entry.path(), d.join(entry.file_name()));
                    }
                }
            }
        }
    } else if let Some(lib) = get("lib_dir") {
        // dev rpath; the shipped layout adds $ORIGIN/python/lib in the app build
        println!("cargo:rustc-link-arg=-Wl,-rpath,{lib}");
    }
}
