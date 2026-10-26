//! A picture of a version and the app that opens it. Pictures come from Quick Look's command-line
//! tool, run on a read-only copy of the version (never on the working file) and kept in a cache.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The picture of `src` as `<cache_dir>/<key>.png`, made once with Quick Look. `None` when Quick
/// Look cannot draw this kind of file, or takes too long.
pub fn thumbnail(src: &Path, cache_dir: &Path, key: &str) -> Option<PathBuf> {
    let out = cache_dir.join(format!("{key}.png"));
    if out.is_file() {
        return Some(out);
    }
    if !src.is_file() {
        return None;
    }
    std::fs::create_dir_all(cache_dir).ok()?;
    let tmp = tempfile::Builder::new().prefix(".ql-").tempdir_in(cache_dir).ok()?;
    if !quick_look(src, tmp.path()) {
        return None;
    }
    let drawn = tmp.path().join(format!("{}.png", src.file_name()?.to_string_lossy()));
    std::fs::rename(&drawn, &out).ok()?;
    Some(out)
}

const QUICK_LOOK: &str = "/usr/bin/qlmanage";
const QUICK_LOOK_TIMEOUT: Duration = Duration::from_secs(5);

/// Run Quick Look's thumbnail tool without a shell; give up after a few seconds.
fn quick_look(src: &Path, out_dir: &Path) -> bool {
    let Ok(mut child) = Command::new(QUICK_LOOK)
        .args(["-t", "-s", "800", "-o"])
        .arg(out_dir)
        .arg(src)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < QUICK_LOOK_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// The name of the app that opens `path` by default ("Microsoft Word", "TextEdit").
#[cfg(target_os = "macos")]
pub fn default_app_name(path: &Path) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    Some(app.URLByDeletingPathExtension()?.lastPathComponent()?.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn default_app_name(_path: &Path) -> Option<String> {
    None
}

/// Open `path` in its default app, as one argument and without a shell.
pub fn open_in_app(path: &Path) -> std::io::Result<()> {
    let status = Command::new("/usr/bin/open").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("open failed"))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn docx(dir: &Path) -> PathBuf {
        let txt = dir.join("t.txt");
        fs::write(&txt, "Teklif: depo otomasyonu\n\nToplam 184.000 TL\n").unwrap();
        let out = dir.join("Teklif_Müşteri.docx");
        let ok = Command::new("textutil")
            .args(["-convert", "docx", "-output"])
            .arg(&out)
            .arg(&txt)
            .status()
            .unwrap();
        assert!(ok.success());
        fs::set_permissions(&out, fs::Permissions::from_mode(0o444)).unwrap();
        out
    }

    #[test]
    fn a_docx_with_a_turkish_name_gets_a_png() {
        let dir = tempfile::tempdir().unwrap();
        let src = docx(dir.path());
        let png = thumbnail(&src, &dir.path().join("cache"), "b3_abc").expect("Quick Look draws a docx");
        assert_eq!(png, dir.path().join("cache").join("b3_abc.png"));
        assert_eq!(&fs::read(&png).unwrap()[..4], b"\x89PNG");
    }

    #[test]
    fn the_second_call_uses_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let src = docx(dir.path());
        let cache = dir.path().join("cache");
        let first = thumbnail(&src, &cache, "k").unwrap();
        let made = fs::metadata(&first).unwrap().modified().unwrap();
        fs::remove_file(&src).unwrap(); // a cached picture needs no source any more
        let second = thumbnail(&src, &cache, "k").unwrap();
        assert_eq!((first, made), (second.clone(), fs::metadata(&second).unwrap().modified().unwrap()));
    }

    #[test]
    fn a_kind_quick_look_cannot_draw_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("x.zzz");
        fs::write(&src, (0..4096u32).map(|i| (i * 7919 % 251) as u8).collect::<Vec<_>>()).unwrap();
        assert_eq!(thumbnail(&src, &dir.path().join("cache"), "z"), None);
        assert!(fs::read_dir(dir.path().join("cache")).map(|d| d.count()).unwrap_or(0) == 0, "no leftovers");
    }

    #[test]
    fn a_text_file_opens_in_some_app() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "x").unwrap();
        let name = default_app_name(&p).expect("every Mac has an app for .txt");
        assert!(!name.is_empty() && !name.ends_with(".app"), "{name}");
    }
}
