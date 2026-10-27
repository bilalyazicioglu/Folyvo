//! What someone asked Folyvo to do from outside its windows: a `folyvo://intent` URL from the
//! Finder extension, or a Services message. Any app or web page can open a `folyvo://` link, so an
//! intent only ever opens a window; it never records, restores, exports or deletes anything.

use std::path::PathBuf;

/// The four things Folyvo offers in Finder and under Services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentKind {
    Browse,
    Send,
    SaveCopy,
    WhichOne,
}

impl IntentKind {
    /// The `kind` in a `folyvo://intent` URL.
    pub fn from_wire(kind: &str) -> Option<IntentKind> {
        match kind {
            "browse" => Some(IntentKind::Browse),
            "send" => Some(IntentKind::Send),
            "save_copy" => Some(IntentKind::SaveCopy),
            "which_one" => Some(IntentKind::WhichOne),
            _ => None,
        }
    }

    /// The `NSMessage` of a Services entry.
    pub fn from_service_message(message: &str) -> Option<IntentKind> {
        match message {
            "browse" => Some(IntentKind::Browse),
            "send" => Some(IntentKind::Send),
            "saveCopy" => Some(IntentKind::SaveCopy),
            "whichOne" => Some(IntentKind::WhichOne),
            _ => None,
        }
    }
}

/// One request about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub kind: IntentKind,
    pub path: PathBuf,
}

/// `folyvo://intent?kind=<kind>&path=<absolute path>`, acting on the first path. Anything else is `None`.
pub fn parse_url(url: &str) -> Option<Intent> {
    let query = url.strip_prefix("folyvo://intent?")?;
    let (mut kind, mut path) = (None, None);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        match key {
            "kind" if kind.is_none() => kind = Some(IntentKind::from_wire(&decode(value)?)?),
            "path" if path.is_none() => path = Some(PathBuf::from(decode(value)?)),
            _ => {}
        }
    }
    let path = path.filter(|p| p.is_absolute())?;
    Some(Intent { kind: kind?, path })
}

/// Whether a link may name `path`: a file inside `home`, written without `.` or `..`. Decided from the
/// text alone, because any web page can open a link, and looking at a path (even to ask whether it
/// exists) can make macOS connect to a network share it names. The comparison ignores case, as the
/// Mac's disk does.
pub fn from_link_allowed(path: &std::path::Path, home: &std::path::Path) -> bool {
    use std::path::Component;
    let plain =
        |p: &std::path::Path| p.components().all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !plain(path) || !plain(home) {
        return false;
    }
    let (path, home) = (path.to_string_lossy().to_lowercase(), home.to_string_lossy().to_lowercase());
    path.strip_prefix(home.trim_end_matches('/')).is_some_and(|rest| rest.len() > 1 && rest.starts_with('/'))
}

/// Whether a Services request may name `path`. The person picked the file, so any local file is fine,
/// another disk included; only another app can fill the pasteboard, so a network share is still
/// refused, decided from the text alone like a link.
pub fn from_service_allowed(path: &std::path::Path) -> bool {
    use std::path::Component;
    let mut parts = path.components();
    if parts.next() != Some(Component::RootDir) || !parts.clone().all(|c| matches!(c, Component::Normal(_))) {
        return false;
    }
    match parts.next() {
        Some(Component::Normal(first)) => {
            let first = first.to_string_lossy().to_lowercase();
            first != "net" && first != "network"
        }
        _ => false,
    }
}

/// Percent-decoding as UTF-8. `+` stays a plus: Finder encodes spaces as `%20`.
fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Which window an intent opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The file's versions window, optionally starting Send a Copy (`send`) or Save a Copy (`save`).
    Versions { item: String, action: Option<&'static str> },
    /// The popover's "which one is this?" answer for a file Folyvo did not save, or when asked.
    Which { path: PathBuf },
}

/// `item` is the saved file at `intent.path`, if Folyvo keeps one there.
pub fn route(intent: &Intent, item: Option<String>) -> Route {
    let action = match intent.kind {
        IntentKind::Browse => None,
        IntentKind::Send => Some("send"),
        IntentKind::SaveCopy => Some("save"),
        IntentKind::WhichOne => return Route::Which { path: intent.path.clone() },
    };
    match item {
        Some(item) => Route::Versions { item, action },
        None => Route::Which { path: intent.path.clone() },
    }
}

/// Intents that arrived before the engine was ready (Folyvo was launched by the request itself).
#[derive(Default)]
pub struct PendingIntent(pub std::sync::Mutex<Option<Intent>>);

/// Opens the window `intent` asks for. Nothing else: no record, restore, export or delete.
pub fn act_on(app: &tauri::AppHandle, intent: Intent) {
    use tauri::Manager;
    let Some(rt) = app.try_state::<std::sync::Arc<crate::runtime::AppRuntime>>() else {
        if let Some(pending) = app.try_state::<PendingIntent>() {
            *pending.0.lock().unwrap() = Some(intent);
        }
        return;
    };
    let item = intent.path.to_str().and_then(|p| rt.api().item_at(p).ok().flatten());
    let _ = match route(&intent, item) {
        Route::Versions { item, action } => crate::tray::open_versions(app, &item, action),
        Route::Which { path } => crate::tray::show_which(app, &path),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_finder_url_names_the_kind_and_the_first_path() {
        let i = parse_url(
            "folyvo://intent?kind=send&path=%2FUsers%2Fa%2FTeklif%20M%C3%BC%C5%9Fteri%20%26%20%23.docx&path=%2Fb",
        )
        .unwrap();
        assert_eq!((i.kind, i.path), (IntentKind::Send, PathBuf::from("/Users/a/Teklif Müşteri & #.docx")));
    }

    #[test]
    fn a_decomposed_name_is_kept_as_it_came() {
        let i = parse_url("folyvo://intent?kind=browse&path=%2Fa%2FMu%CC%88s.docx").unwrap();
        assert_eq!(i.path, PathBuf::from("/a/Mu\u{308}s.docx"));
    }

    #[test]
    fn anything_else_is_ignored() {
        for url in [
            "folyvo://intent?kind=delete&path=%2Fa", // unknown kind
            "folyvo://intent?kind=send&path=a%2Fb",  // relative path
            "folyvo://intent?kind=send",             // no path
            "folyvo://other?kind=send&path=%2Fa",    // other host
            "https://intent?kind=send&path=%2Fa",    // other scheme
            "folyvo://intent?kind=send&path=%FF%FE", // not UTF-8
        ] {
            assert_eq!(parse_url(url), None, "{url}");
        }
    }

    #[test]
    fn a_link_may_only_name_a_file_in_the_home_folder() {
        let home = Path::new("/Users/a");
        for ok in ["/Users/a/Desktop/Teklif.docx", "/USERS/A/Library/Mobile Documents/x.txt", "/Users/a/x"] {
            assert!(from_link_allowed(Path::new(ok), home), "{ok}");
        }
        for no in [
            "/net/host/share/x.docx", // makes macOS connect to the host
            "/Network/Servers/x",
            "/Volumes/USB/x.docx",       // another disk
            "/Users/a/../../net/host/x", // climbs out of home
            "/Users/a/./../b/x",
            "/Users/ab/x", // another folder that only starts the same
            "/Users/b/x",
            "/tmp/x",
            "/Users/a", // home itself is not a file
        ] {
            assert!(!from_link_allowed(Path::new(no), home), "{no}");
        }
    }

    #[test]
    fn services_take_any_local_file_but_not_a_network_share() {
        for ok in ["/Users/a/Desktop/x.docx", "/Volumes/USB/Teklif.docx", "/tmp/x"] {
            assert!(from_service_allowed(Path::new(ok)), "{ok}");
        }
        for no in
            ["/net/host/x", "/NET/host/x", "/Network/Servers/x", "/Users/a/../../net/h/x", "relative/x", "/"]
        {
            assert!(!from_service_allowed(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn services_messages_name_the_same_kinds() {
        assert_eq!(IntentKind::from_service_message("saveCopy"), Some(IntentKind::SaveCopy));
        assert_eq!(IntentKind::from_service_message("whichOne"), Some(IntentKind::WhichOne));
        assert_eq!(IntentKind::from_service_message("rm"), None);
    }

    #[test]
    fn a_saved_file_opens_its_versions_and_anything_else_asks_which_one() {
        let at = |kind| Intent { kind, path: PathBuf::from("/a/x.docx") };
        assert_eq!(
            route(&at(IntentKind::Browse), Some("itm_1".into())),
            Route::Versions { item: "itm_1".into(), action: None }
        );
        assert_eq!(
            route(&at(IntentKind::Send), Some("itm_1".into())),
            Route::Versions { item: "itm_1".into(), action: Some("send") }
        );
        assert_eq!(
            route(&at(IntentKind::SaveCopy), Some("itm_1".into())),
            Route::Versions { item: "itm_1".into(), action: Some("save") }
        );
        for kind in [IntentKind::Browse, IntentKind::Send, IntentKind::SaveCopy, IntentKind::WhichOne] {
            assert_eq!(route(&at(kind), None), Route::Which { path: PathBuf::from("/a/x.docx") });
        }
        assert_eq!(
            route(&at(IntentKind::WhichOne), Some("itm_1".into())),
            Route::Which { path: PathBuf::from("/a/x.docx") }
        );
    }
}
