//! Install pipeline and PyPI pair tests against a local server.
use crate::fetch::{Ctx, extract, hex};
use crate::matrix::{Archive, Target};
use crate::testutil::{Route, serve};
use crate::ytdlp::{check_with, install_with};
use crate::{Dirs, Tool, install, pair_ffmpeg};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, d) in entries {
        w.start_file(*n, o).unwrap();
        w.write_all(d).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn ctx(base: &str, target: Target) -> Ctx {
    let mut c = Ctx::new().unwrap();
    c.mirror = Some(base.to_string());
    c.target = target;
    c.fake_version = Some("9.0-test".into());
    c
}

fn dirs(root: &Path) -> Dirs {
    Dirs {
        data: root.join("data"),
        cache: root.join("cache"),
    }
}

fn count(p: &Path) -> usize {
    std::fs::read_dir(p).map(|r| r.count()).unwrap_or(0)
}

const WIN: Target = Target::ALL[0];
const MAC_ARM: Target = Target::ALL[3];

#[tokio::test]
async fn btbn_install_and_progress() {
    let z = zip_of(&[
        ("ffmpeg-n9.0-x/bin/ffmpeg.exe", b"FF"),
        ("ffmpeg-n9.0-x/bin/ffprobe.exe", b"FP"),
        ("ffmpeg-n9.0-x/doc/ffmpeg.exe", b"no"),
    ]);
    let name = "ffmpeg-n9.0-latest-win64-gpl-9.0.zip";
    let sums = format!(
        "{}  ffmpeg-n8.1-latest-win64-gpl-8.1.zip\n{} *{name}\n",
        "0".repeat(64),
        sha(&z)
    );
    let base = serve(vec![
        (
            "/btbn/checksums.sha256".into(),
            Route::Body(sums.into_bytes()),
        ),
        (format!("/btbn/{name}"), Route::Body(z)),
    ]);
    let t = tempfile::tempdir().unwrap();
    let d = dirs(t.path());
    let seen = std::sync::atomic::AtomicU64::new(0);
    let s = install(&ctx(&base, WIN), Tool::Ffmpeg, &d, &|done, total| {
        assert!(total.is_some());
        seen.store(done, std::sync::atomic::Ordering::SeqCst);
    })
    .await
    .unwrap();
    assert_eq!(s.version, "9.0-test");
    assert_eq!(std::fs::read(&s.path).unwrap(), b"FF");
    assert_eq!(
        std::fs::read(
            d.data
                .join("tools/ffprobe/9.0-test")
                .join(Tool::Ffprobe.exe_name())
        )
        .unwrap(),
        b"FP"
    );
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst) > 0);
    // no staging or download leftovers
    assert_eq!(count(&d.data.join("tools")), 2);
    assert_eq!(count(&d.cache.join("tools")), 0);
}

#[tokio::test]
async fn tampered_archive_rejected_and_cleaned() {
    let z = zip_of(&[("bin/ffmpeg.exe", b"FF"), ("bin/ffprobe.exe", b"FP")]);
    let name = "ffmpeg-n9.0-latest-win64-gpl-9.0.zip";
    let sums = format!("{}  {name}\n", sha(b"something else"));
    let base = serve(vec![
        (
            "/btbn/checksums.sha256".into(),
            Route::Body(sums.into_bytes()),
        ),
        (format!("/btbn/{name}"), Route::Body(z)),
    ]);
    let t = tempfile::tempdir().unwrap();
    let d = dirs(t.path());
    let e = install(&ctx(&base, WIN), Tool::Ffmpeg, &d, &|_, _| {})
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("checksum mismatch"), "{e:#}");
    assert_eq!(count(&d.data.join("tools")), 0);
    assert_eq!(count(&d.cache.join("tools")), 0);
}

#[tokio::test]
async fn missing_or_garbage_checksum_fails_closed() {
    let z = zip_of(&[("deno.exe", b"D")]);
    let url = "/deno/deno-x86_64-pc-windows-msvc.zip";
    for sums in [None, Some(b"<html>nope</html>".to_vec())] {
        let mut routes = vec![(url.to_string(), Route::Body(z.clone()))];
        if let Some(s) = sums {
            routes.push((format!("{url}.sha256sum"), Route::Body(s)));
        }
        let base = serve(routes);
        let t = tempfile::tempdir().unwrap();
        let d = dirs(t.path());
        assert!(
            install(&ctx(&base, WIN), Tool::Deno, &d, &|_, _| {})
                .await
                .is_err()
        );
        assert_eq!(count(&d.data.join("tools")), 0);
        assert_eq!(count(&d.cache.join("tools")), 0);
    }
}

#[tokio::test]
async fn interrupted_stream_leaves_nothing() {
    let z = zip_of(&[("deno.exe", b"D")]);
    let url = "/deno/deno-x86_64-pc-windows-msvc.zip";
    let base = serve(vec![
        (url.into(), Route::Truncated(z.clone())),
        (
            format!("{url}.sha256sum"),
            Route::Body(sha(&z).into_bytes()),
        ),
    ]);
    let t = tempfile::tempdir().unwrap();
    let d = dirs(t.path());
    assert!(
        install(&ctx(&base, WIN), Tool::Deno, &d, &|_, _| {})
            .await
            .is_err()
    );
    assert_eq!(count(&d.data.join("tools")), 0);
    assert_eq!(count(&d.cache.join("tools")), 0);
}

#[tokio::test]
async fn deno_powershell_checksum_installs() {
    let z = zip_of(&[("deno.exe", b"D")]);
    let url = "/deno/deno-x86_64-pc-windows-msvc.zip";
    let ps = format!(
        "\nAlgorithm : SHA256\nHash      : {}\nPath      : C:\\a\\deno.zip\n",
        sha(&z).to_uppercase()
    );
    let base = serve(vec![
        (url.into(), Route::Body(z)),
        (format!("{url}.sha256sum"), Route::Body(ps.into_bytes())),
    ]);
    let t = tempfile::tempdir().unwrap();
    let s = install(&ctx(&base, WIN), Tool::Deno, &dirs(t.path()), &|_, _| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(s.path).unwrap(), b"D");
}

#[tokio::test]
async fn mac_redirect_then_checksum_at_resolved_url() {
    let z = zip_of(&[("ffprobe", b"P")]);
    let r = "/martin-riedl/redirect/latest/macos/arm64/release/ffprobe.zip";
    let fin = "/martin-riedl/download/macos/arm64/1795_9.1/ffprobe.zip";
    let base = serve(vec![
        (r.into(), Route::Redirect(fin.into())),
        (fin.into(), Route::Body(z.clone())),
        (
            format!("{fin}.sha256"),
            Route::Body(format!("{}  ffprobe.zip\n", sha(&z)).into_bytes()),
        ),
    ]);
    let t = tempfile::tempdir().unwrap();
    let s = install(
        &ctx(&base, MAC_ARM),
        Tool::Ffprobe,
        &dirs(t.path()),
        &|_, _| {},
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(s.path).unwrap(), b"P");
}

#[test]
fn zip_slip_rejected() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("a.zip");
    std::fs::write(
        &z,
        zip_of(&[("bin/ffmpeg.exe", b"F"), ("../evil.txt", b"x")]),
    )
    .unwrap();
    let out = t.path().join("out");
    let e = extract(&z, Archive::Zip, &[Tool::Ffmpeg], &out).unwrap_err();
    assert!(e.to_string().contains("unsafe path"), "{e}");
    assert!(!t.path().join("evil.txt").exists());
    // wheel unzip shares the guard
    std::fs::write(&z, zip_of(&[("..\\evil.txt", b"x")])).unwrap();
    assert!(crate::fetch::unzip_wheel(&z, &out, 1 << 20).is_err());
}

fn tar_with(entries: Vec<(&str, Option<&str>, &[u8])>) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (name, link, data) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_mode(0o755);
        if let Some(l) = link {
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            h.set_link_name(l).unwrap();
            h.set_cksum();
            b.append_data(&mut h, name, std::io::empty()).unwrap();
        } else {
            h.set_size(data.len() as u64);
            h.set_cksum();
            b.append_data(&mut h, name, data).unwrap();
        }
    }
    b.into_inner().unwrap()
}

#[test]
fn tar_xz_gz_extract_and_symlink_guard() {
    let t = tempfile::tempdir().unwrap();
    let raw = tar_with(vec![
        ("ffmpeg-n9.0/bin/ffmpeg", None, b"F"),
        ("ffmpeg-n9.0/bin/ffprobe", None, b"P"),
        ("ffmpeg-n9.0/lib/link", Some("/etc/passwd"), b""),
    ]);
    let xz = t.path().join("a.tar.xz");
    let mut e = liblzma::write::XzEncoder::new(std::fs::File::create(&xz).unwrap(), 1);
    e.write_all(&raw).unwrap();
    e.finish().unwrap();
    let gz = t.path().join("a.tar.gz");
    let mut e = flate2::write::GzEncoder::new(
        std::fs::File::create(&gz).unwrap(),
        flate2::Compression::fast(),
    );
    e.write_all(&raw).unwrap();
    e.finish().unwrap();
    for (f, k) in [(xz, Archive::TarXz), (gz, Archive::TarGz)] {
        let out = t.path().join(format!("out-{k:?}"));
        extract(&f, k, &[Tool::Ffmpeg, Tool::Ffprobe], &out).unwrap();
        assert_eq!(
            std::fs::read(out.join("ffprobe").join(Tool::Ffprobe.exe_name())).unwrap(),
            b"P"
        );
    }
    // a symlink in place of the binary is an error
    let bad = tar_with(vec![("x/bin/ffmpeg", Some("/bin/sh"), b"")]);
    let f = t.path().join("bad.tar.gz");
    let mut e = flate2::write::GzEncoder::new(
        std::fs::File::create(&f).unwrap(),
        flate2::Compression::fast(),
    );
    e.write_all(&bad).unwrap();
    e.finish().unwrap();
    assert!(extract(&f, Archive::TarGz, &[Tool::Ffmpeg], &t.path().join("o2")).is_err());
}

// ---- yt-dlp pair ----

fn wheel(name: &str, version: &str, extra_md: &str, body: &str) -> Vec<u8> {
    let md = format!("Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n{extra_md}");
    let pkg = name.replace('-', "_");
    zip_of(&[
        (&format!("{pkg}/__init__.py"), body.as_bytes()),
        (
            &format!("{pkg}-{version}.dist-info/METADATA"),
            md.as_bytes(),
        ),
    ])
}

/// Serve yt-dlp `ver` (METADATA `pin_md`) and ejs 0.8.0; `ejs_sha` / `ejs_body` allow corruption.
fn pypi(ver: &str, pin_md: &str, ejs_sha: Option<String>, ejs_body: Vec<u8>) -> String {
    let yw = wheel("yt-dlp", ver, pin_md, "x = 1");
    let ew_sha = ejs_sha.unwrap_or_else(|| sha(&ejs_body));
    let (l, base) = crate::testutil::bind();
    let doc = |name: &str, v: &str, file: &str, h: &str| {
        serde_json::json!({"info": {"version": v}, "releases": {v: [{"packagetype": "bdist_wheel", "filename": format!("{name}-{v}-py3-none-any.whl"), "url": format!("{base}/files/{file}"), "digests": {"sha256": h}}]}}).to_string()
    };
    let routes = vec![
        (
            "/pypi/yt-dlp/json".to_string(),
            Route::Body(doc("yt_dlp", ver, "y.whl", &sha(&yw)).into_bytes()),
        ),
        (
            "/pypi/yt-dlp-ejs/json".to_string(),
            Route::Body(doc("yt_dlp_ejs", "0.8.0", "e.whl", &ew_sha).into_bytes()),
        ),
        ("/files/y.whl".to_string(), Route::Body(yw)),
        ("/files/e.whl".to_string(), Route::Body(ejs_body)),
    ];
    crate::testutil::serve_on(l, routes);
    base
}

const PIN: &str = "Requires-Dist: yt-dlp-ejs (==0.8.0) ; extra == \"default\"\n";

#[tokio::test]
async fn pair_update_and_install_with_rollback_pointer() {
    let ejs = wheel("yt-dlp-ejs", "0.8.0", "", "e = 1");
    let p = pypi("2026.11.1", PIN, None, ejs);
    let c = ctx(&p, WIN);
    let t = tempfile::tempdir().unwrap();
    let d = dirs(t.path());
    assert!(
        check_with(&c, &d, Some("2026.11.1"))
            .await
            .unwrap()
            .is_none()
    );
    let up = check_with(&c, &d, Some("2026.10.26"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (up.ytdlp.version.as_str(), up.ejs.version.as_str()),
        ("2026.11.1", "0.8.0")
    );
    let old = d.data.join("python-site/2026.10.26");
    std::fs::create_dir_all(&old).unwrap();
    let ok = install_with(&c, &d, &up, Some(&old), |dir| {
        assert!(dir.ends_with("2026.11.1.tmp"));
        assert!(
            dir.join("yt_dlp/__init__.py").is_file()
                && dir.join("yt_dlp_ejs/__init__.py").is_file()
        );
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(ok.active, d.data.join("python-site/2026.11.1"));
    assert_eq!(ok.previous.as_deref(), Some(old.as_path()));
    assert!(ok.active.join("yt_dlp/__init__.py").is_file() && old.is_dir());
    assert_eq!(count(&d.data.join("python-site")), 2);
}

#[tokio::test]
async fn pair_corrupt_cases_leave_pointer_unchanged() {
    let good_ejs = wheel("yt-dlp-ejs", "0.8.0", "", "e = 1");
    let t = tempfile::tempdir().unwrap();
    let d = dirs(t.path());
    let site = d.data.join("python-site");
    let mk = |p: &String| ctx(p, WIN);

    // 1. ejs digest does not match its bytes
    let p = pypi("2026.11.1", PIN, Some("0".repeat(64)), good_ejs.clone());
    let c = mk(&p);
    let up = check_with(&c, &d, None).await.unwrap().unwrap();
    assert!(install_with(&c, &d, &up, None, |_| Ok(())).await.is_err());
    assert_eq!(count(&site), 0);

    // 2. ejs is not a zip (digest matches)
    let p = pypi("2026.11.1", PIN, None, b"not a zip".to_vec());
    let c = mk(&p);
    let up = check_with(&c, &d, None).await.unwrap().unwrap();
    assert!(install_with(&c, &d, &up, None, |_| Ok(())).await.is_err());
    assert_eq!(count(&site), 0);

    // 3. yt-dlp wheel without a pin: the check fails
    let p = pypi("2026.11.1", "", None, good_ejs.clone());
    assert!(check_with(&mk(&p), &d, None).await.is_err());

    // 4. verifier rejects: .tmp removed, no final dir
    let p = pypi("2026.11.1", PIN, None, good_ejs);
    let c = mk(&p);
    let up = check_with(&c, &d, None).await.unwrap().unwrap();
    let e = install_with(&c, &d, &up, None, |_| anyhow::bail!("boom"))
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("boom"));
    assert_eq!(count(&site), 0);
}

#[test]
fn pair_ffmpeg_puts_each_binary_next_to_the_other() {
    let root = tempfile::tempdir().unwrap();
    let d = dirs(root.path());
    let tools = root.path().join("data/tools");
    let place = |t: Tool, body: &[u8]| {
        let dir = tools.join(t.id()).join("9.1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(t.exe_name()), body).unwrap();
        dir
    };
    let ff = place(Tool::Ffmpeg, b"FF");
    let fp = place(Tool::Ffprobe, b"PROBE");
    pair_ffmpeg(&d);
    assert_eq!(
        std::fs::read(ff.join(Tool::Ffprobe.exe_name())).unwrap(),
        b"PROBE"
    );
    assert_eq!(
        std::fs::read(fp.join(Tool::Ffmpeg.exe_name())).unwrap(),
        b"FF"
    );
    pair_ffmpeg(&d); // idempotent
}

#[test]
fn version_parsing() {
    use crate::parse_version as pv;
    assert_eq!(
        pv(
            "ffmpeg",
            "ffmpeg version 7.0.1-essentials_build-www.gyan.dev Copyright"
        )
        .as_deref(),
        Some("7.0.1-essentials_build-www.gyan.dev")
    );
    assert_eq!(
        pv("ffprobe", "ffprobe version n9.0-3-gabc Copyright").as_deref(),
        Some("n9.0-3-gabc")
    );
    assert_eq!(pv("ffprobe", "ffmpeg version 7.0 x"), None);
    assert_eq!(
        pv(
            "deno",
            "deno 2.9.8 (stable, release, x86_64-pc-windows-msvc)\nv8 14"
        )
        .as_deref(),
        Some("2.9.8")
    );
    assert_eq!(pv("node", "v22.3.0\n").as_deref(), Some("22.3.0"));
    assert_eq!(pv("ffmpeg", "hello"), None);
    assert!(
        crate::at_least("22.3.0", (22, 0))
            && !crate::at_least("2.2.9", (2, 3))
            && crate::at_least("2.9.8", (2, 3))
    );
}

#[test]
fn js_dict_shape() {
    let r = crate::JsRuntime {
        name: "deno",
        path: "/x/deno".into(),
        version: "2.9.8".into(),
    };
    assert_eq!(
        crate::js_runtimes_dict(&[r]),
        serde_json::json!({"deno": {"path": "/x/deno"}})
    );
}
