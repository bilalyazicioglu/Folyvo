//! A picture of a version and the app that opens it. Pictures come from Quick Look's command-line
//! tool, run on a read-only copy of the version (never on the working file) and kept in a cache.

use std::io::Read;
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

/// Kinds shown as their own words. Quick Look draws plain text dark-on-dark in the dark appearance.
const TEXT_KINDS: &[&str] =
    &["txt", "text", "md", "markdown", "csv", "tsv", "json", "log", "xml", "yml", "yaml"];
/// The most of a text file that is shown: its first lines, and never more than this many bytes.
pub const EXCERPT_LINES: usize = 60;
pub const EXCERPT_BYTES: usize = 16 * 1024;

/// The first lines of a plain text file, or `None` when `src` is not one or is not UTF-8 text.
pub fn text_excerpt(src: &Path) -> Option<String> {
    let ext = src.extension()?.to_str()?.to_ascii_lowercase();
    if !TEXT_KINDS.contains(&ext.as_str()) {
        return None;
    }
    let mut head = Vec::with_capacity(EXCERPT_BYTES);
    std::fs::File::open(src).ok()?.take(EXCERPT_BYTES as u64).read_to_end(&mut head).ok()?;
    let text = match std::str::from_utf8(&head) {
        Ok(t) => t,
        // A character cut by the byte limit is dropped; anything else is not text.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    if text.contains('\0') {
        return None;
    }
    let end = text.match_indices('\n').nth(EXCERPT_LINES - 1).map_or(text.len(), |(i, _)| i + 1);
    Some(text[..end].to_string())
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
    fn a_text_file_is_shown_as_its_own_words() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Toplantı notları.txt");
        fs::write(&p, "Fiyatlar güncellendi\nçarşamba 10:12\n").unwrap();
        assert_eq!(text_excerpt(&p).as_deref(), Some("Fiyatlar güncellendi\nçarşamba 10:12\n"));
    }

    #[test]
    fn a_long_text_file_is_cut_after_its_first_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("log.md");
        fs::write(&p, "satır ğüşiöç\n".repeat(5_000)).unwrap();
        let text = text_excerpt(&p).unwrap();
        assert_eq!(text.lines().count(), EXCERPT_LINES);
        assert!(text.len() <= EXCERPT_BYTES);
    }

    #[test]
    fn only_plain_text_is_read_as_words() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("x.txt");
        fs::write(&binary, [b'a', 0, 0xff, 0xfe, b'b']).unwrap();
        assert_eq!(text_excerpt(&binary), None, "a .txt full of bytes is not text");
        let src = docx(dir.path());
        assert_eq!(text_excerpt(&src), None, "a docx is drawn by Quick Look");
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
