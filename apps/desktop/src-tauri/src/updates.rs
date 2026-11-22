//! Updates from inside the app (#52). Once a day Folyvo asks for the latest version and says
//! when a newer one exists; nothing is installed until the person asks.

use std::path::Path;
use std::sync::{Arc, Mutex};

use folyvo_core::engine::Engine;
use folyvo_core::ports::Clock;

use crate::dto::{EngineEventDto, UpdatesDto};
use crate::errors::UserError;
use crate::runtime::Notifier;

/// How long a successful look lasts. A look that fails, as offline, is tried again sooner.
pub const CHECK_EVERY_MS: i64 = 24 * 60 * 60 * 1_000;
const CHECKS: &str = "update_checks";
const LAST_CHECK: &str = "last_update_check";

/// Where newer versions come from.
pub trait Releases: Send + Sync {
    /// The newer version, if there is one. An error means the question went unanswered.
    fn newer(&self) -> Result<Option<Arc<dyn Release>>, String>;
}

pub trait Release: Send + Sync {
    fn version(&self) -> String;
    /// Downloads it, checks its signature and puts it in place of the running app.
    fn install(&self) -> Result<(), String>;
}

/// Whether the running app can be replaced where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Updatable,
    /// Run from a place it cannot replace itself in: macOS's App Translocation, which runs a
    /// downloaded app from a read-only copy until it is moved, or the disk image.
    NeedsMove,
    /// Not inside an app bundle: a development build, which is never updated.
    Development,
}

impl Placement {
    /// From the running executable, `…/Folyvo.app/Contents/MacOS/folyvo-desktop`.
    pub fn of(exe: &Path) -> Placement {
        let Some(bundle) = exe.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app"))
        else {
            return Placement::Development;
        };
        let translocated = bundle.components().any(|c| c.as_os_str() == "AppTranslocation");
        // The new app is put in with renames next to the old one, so its folder must take a file.
        let writable = bundle.parent().is_some_and(|dir| tempfile::tempfile_in(dir).is_ok());
        if translocated || !writable {
            Placement::NeedsMove
        } else {
            Placement::Updatable
        }
    }
}

enum State {
    Idle,
    Ready(Arc<dyn Release>),
    Installing(Arc<dyn Release>),
    Failed(Arc<dyn Release>),
}

pub struct Updates {
    engine: Arc<Engine>,
    clock: Arc<dyn Clock>,
    releases: Box<dyn Releases>,
    placement: Placement,
    notifier: Arc<dyn Notifier>,
    current: String,
    state: Mutex<State>,
}

impl Updates {
    pub fn new(
        engine: Arc<Engine>,
        clock: Arc<dyn Clock>,
        releases: Box<dyn Releases>,
        placement: Placement,
        notifier: Arc<dyn Notifier>,
        current: &str,
    ) -> Updates {
        Updates {
            engine,
            clock,
            releases,
            placement,
            notifier,
            current: current.into(),
            state: Mutex::new(State::Idle),
        }
    }

    pub fn status(&self) -> Result<UpdatesDto, UserError> {
        let (ready, installing, failed) = match &*self.state.lock().unwrap() {
            State::Idle => (None, false, false),
            State::Ready(r) => (Some(r.version()), false, false),
            State::Installing(r) => (Some(r.version()), true, false),
            State::Failed(r) => (Some(r.version()), false, true),
        };
        Ok(UpdatesDto {
            checks: self.checks()?,
            current: self.current.clone(),
            ready,
            installing,
            failed,
            needs_move: self.placement == Placement::NeedsMove,
        })
    }

    fn checks(&self) -> Result<bool, UserError> {
        Ok(self.engine.setting(CHECKS)?.as_deref() != Some("off"))
    }

    /// Asks for the latest version when the checks are on and the last answer is a day old.
    pub fn check_if_due(&self) {
        if self.placement != Placement::Updatable || !matches!(*self.state.lock().unwrap(), State::Idle) {
            return;
        }
        let last = self.engine.setting(LAST_CHECK).ok().flatten().and_then(|v| v.parse::<i64>().ok());
        let now = self.clock.now_unix_ms();
        if !self.checks().unwrap_or(false) || last.is_some_and(|at| now - at < CHECK_EVERY_MS) {
            return;
        }
        match self.releases.newer() {
            Ok(found) => {
                if let Err(e) = self.engine.set_setting(LAST_CHECK, &now.to_string()) {
                    eprintln!("folyvo: could not note the look for updates: {e}");
                }
                if let Some(release) = found {
                    *self.state.lock().unwrap() = State::Ready(release);
                    self.notifier.notify(EngineEventDto::UpdateChanged);
                }
            }
            Err(e) => eprintln!("folyvo: could not look for updates: {e}"),
        }
    }

    pub fn set_checks(&self, on: bool) -> Result<(), UserError> {
        self.engine.set_setting(CHECKS, if on { "on" } else { "off" })?;
        if !on {
            let mut state = self.state.lock().unwrap();
            if !matches!(*state, State::Installing(_)) {
                *state = State::Idle;
            }
        }
        self.notifier.notify(EngineEventDto::UpdateChanged);
        Ok(())
    }

    /// Installs the version found. On success the caller restarts Folyvo.
    pub fn install(&self) -> Result<(), UserError> {
        let release = {
            let mut state = self.state.lock().unwrap();
            let release = match &*state {
                State::Ready(r) | State::Failed(r) => r.clone(),
                State::Installing(_) => return Ok(()),
                State::Idle => {
                    return Err(UserError::bad_request(
                        "There is no update to install",
                        "Folyvo is up to date.",
                    ))
                }
            };
            *state = State::Installing(release.clone());
            release
        };
        self.notifier.notify(EngineEventDto::UpdateChanged);
        let done = release.install();
        if let Err(e) = &done {
            eprintln!("folyvo: could not install {}: {e}", release.version());
            *self.state.lock().unwrap() = State::Failed(release);
            self.notifier.notify(EngineEventDto::UpdateChanged);
        }
        done.map_err(|_| {
            UserError::new(
                "update_failed",
                "Folyvo could not install the update",
                "Nothing was changed. Check the connection and try again.",
            )
        })
    }
}

/// Newer versions from the endpoint in tauri.conf.json, or from `FOLYVO_UPDATES_URL` when set, to
/// try an update end to end against a local server. Either way the signature is checked against
/// the public key built into the app.
pub struct TauriReleases(pub tauri::AppHandle);

impl Releases for TauriReleases {
    fn newer(&self) -> Result<Option<Arc<dyn Release>>, String> {
        use tauri_plugin_updater::UpdaterExt;
        let mut builder = self.0.updater_builder();
        if let Some(url) = std::env::var("FOLYVO_UPDATES_URL").ok().filter(|u| !u.is_empty()) {
            let url = url.parse().map_err(|e| format!("{e}"))?;
            builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
        }
        let updater = builder.build().map_err(|e| e.to_string())?;
        let found = tauri::async_runtime::block_on(updater.check()).map_err(|e| e.to_string())?;
        Ok(found.map(|update| Arc::new(TauriRelease(update)) as Arc<dyn Release>))
    }
}

struct TauriRelease(tauri_plugin_updater::Update);

impl Release for TauriRelease {
    fn version(&self) -> String {
        self.0.version.clone()
    }

    fn install(&self) -> Result<(), String> {
        tauri::async_runtime::block_on(self.0.download_and_install(|_, _| {}, || {}))
            .map_err(|e| e.to_string())
    }
}

/// Looks for a newer version a while after launch, so the check of kept folders goes first, then
/// every hour; `check_if_due` itself asks at most once a day.
pub fn look_in_background(updates: Arc<Updates>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(30));
        loop {
            updates.check_if_due();
            std::thread::sleep(std::time::Duration::from_secs(60 * 60));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use folyvo_core::ports::{FakeClock, SeqIdGen};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Shelf {
        asked: AtomicUsize,
        installs: AtomicUsize,
        newer: Mutex<Option<&'static str>>,
        offline: Mutex<bool>,
        install_fails: Mutex<bool>,
    }

    struct FakeReleases(Arc<Shelf>);
    struct FakeRelease(Arc<Shelf>, String);

    impl Releases for FakeReleases {
        fn newer(&self) -> Result<Option<Arc<dyn Release>>, String> {
            self.0.asked.fetch_add(1, Ordering::SeqCst);
            if *self.0.offline.lock().unwrap() {
                return Err("offline".into());
            }
            Ok(self
                .0
                .newer
                .lock()
                .unwrap()
                .map(|v| Arc::new(FakeRelease(self.0.clone(), v.into())) as Arc<dyn Release>))
        }
    }

    impl Release for FakeRelease {
        fn version(&self) -> String {
            self.1.clone()
        }
        fn install(&self) -> Result<(), String> {
            self.0.installs.fetch_add(1, Ordering::SeqCst);
            if *self.0.install_fails.lock().unwrap() {
                Err("broken download".into())
            } else {
                Ok(())
            }
        }
    }

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Notifier for Count {
        fn notify(&self, event: EngineEventDto) {
            assert_eq!(event, EngineEventDto::UpdateChanged);
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct T {
        _dir: tempfile::TempDir,
        shelf: Arc<Shelf>,
        clock: Arc<FakeClock>,
        told: Arc<Count>,
        updates: Updates,
    }

    fn setup(placement: Placement) -> T {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(FakeClock::new(1_000_000));
        let engine = Arc::new(
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap(),
        );
        let shelf = Arc::new(Shelf::default());
        let told = Arc::new(Count::default());
        let updates = Updates::new(
            engine,
            clock.clone(),
            Box::new(FakeReleases(shelf.clone())),
            placement,
            told.clone(),
            "0.0.1",
        );
        T { _dir: dir, shelf, clock, told, updates }
    }

    impl T {
        fn asked(&self) -> usize {
            self.shelf.asked.load(Ordering::SeqCst)
        }
        fn publish(&self, v: &'static str) {
            *self.shelf.newer.lock().unwrap() = Some(v);
        }
    }

    #[test]
    fn checks_are_on_until_turned_off_and_nothing_is_found_at_first() {
        let t = setup(Placement::Updatable);
        let s = t.updates.status().unwrap();
        assert_eq!(
            s,
            UpdatesDto {
                checks: true,
                current: "0.0.1".into(),
                ready: None,
                installing: false,
                failed: false,
                needs_move: false
            }
        );
    }

    #[test]
    fn a_newer_version_is_looked_for_once_a_day() {
        let t = setup(Placement::Updatable);
        t.updates.check_if_due();
        t.updates.check_if_due();
        assert_eq!(t.asked(), 1);
        t.clock.advance(CHECK_EVERY_MS - 1);
        t.updates.check_if_due();
        assert_eq!(t.asked(), 1);
        t.clock.advance(1);
        t.updates.check_if_due();
        assert_eq!(t.asked(), 2);
    }

    #[test]
    fn a_look_that_went_unanswered_is_tried_again_at_the_next_turn() {
        let t = setup(Placement::Updatable);
        *t.shelf.offline.lock().unwrap() = true;
        t.updates.check_if_due();
        t.updates.check_if_due();
        assert_eq!(t.asked(), 2);
        assert_eq!(t.updates.status().unwrap().ready, None);
    }

    #[test]
    fn the_day_counts_from_the_last_answer_even_after_folyvo_opens_again() {
        let t = setup(Placement::Updatable);
        t.updates.check_if_due();
        let again = Updates::new(
            t.updates.engine.clone(),
            t.clock.clone(),
            Box::new(FakeReleases(t.shelf.clone())),
            Placement::Updatable,
            t.told.clone(),
            "0.0.1",
        );
        again.check_if_due();
        assert_eq!(t.asked(), 1);
    }

    #[test]
    fn a_newer_version_found_is_told_and_waits_to_be_installed() {
        let t = setup(Placement::Updatable);
        t.publish("0.0.2");
        t.updates.check_if_due();
        assert_eq!(t.updates.status().unwrap().ready.as_deref(), Some("0.0.2"));
        assert_eq!(t.told.0.load(Ordering::SeqCst), 1);
        assert_eq!(t.shelf.installs.load(Ordering::SeqCst), 0, "nothing installs unasked");
    }

    #[test]
    fn nothing_is_looked_for_when_checks_are_off() {
        let t = setup(Placement::Updatable);
        t.updates.set_checks(false).unwrap();
        t.updates.check_if_due();
        assert_eq!(t.asked(), 0);
        assert!(!t.updates.status().unwrap().checks);
        t.updates.set_checks(true).unwrap();
        t.updates.check_if_due();
        assert_eq!(t.asked(), 1);
    }

    #[test]
    fn turning_checks_off_forgets_the_version_found() {
        let t = setup(Placement::Updatable);
        t.publish("0.0.2");
        t.updates.check_if_due();
        t.updates.set_checks(false).unwrap();
        assert_eq!(t.updates.status().unwrap().ready, None);
    }

    #[test]
    fn a_development_build_never_looks() {
        let t = setup(Placement::Development);
        t.updates.check_if_due();
        assert_eq!(t.asked(), 0);
        assert!(!t.updates.status().unwrap().needs_move);
    }

    #[test]
    fn an_app_that_cannot_replace_itself_does_not_look_and_says_to_move_it() {
        let t = setup(Placement::NeedsMove);
        t.updates.check_if_due();
        assert_eq!(t.asked(), 0);
        assert!(t.updates.status().unwrap().needs_move);
    }

    #[test]
    fn installing_puts_in_the_version_found() {
        let t = setup(Placement::Updatable);
        t.publish("0.0.2");
        t.updates.check_if_due();
        t.updates.install().unwrap();
        assert_eq!(t.shelf.installs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn installing_with_nothing_found_is_an_error_not_a_panic() {
        let t = setup(Placement::Updatable);
        assert!(t.updates.install().is_err());
    }

    #[test]
    fn a_failed_install_can_be_tried_again() {
        let t = setup(Placement::Updatable);
        t.publish("0.0.2");
        t.updates.check_if_due();
        *t.shelf.install_fails.lock().unwrap() = true;
        let problem = t.updates.install().unwrap_err();
        assert_eq!(problem.code, "update_failed");
        let s = t.updates.status().unwrap();
        assert!(s.failed && !s.installing);
        assert_eq!(s.ready.as_deref(), Some("0.0.2"));
        *t.shelf.install_fails.lock().unwrap() = false;
        t.updates.install().unwrap();
        assert_eq!(t.shelf.installs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_version_found_is_not_asked_for_again_until_it_is_installed() {
        let t = setup(Placement::Updatable);
        t.publish("0.0.2");
        t.updates.check_if_due();
        t.clock.advance(CHECK_EVERY_MS);
        t.updates.check_if_due();
        assert_eq!(t.asked(), 1);
    }

    /// Without this section the app does not start, and a `{{…}}` in the address would tell the
    /// server about this Mac.
    #[test]
    fn the_app_asks_one_fixed_address_and_knows_the_key_updates_are_signed_with() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert!(updater["pubkey"].as_str().is_some_and(|k| k.len() > 40));
        let endpoints = updater["endpoints"].as_array().unwrap();
        assert_eq!(
            endpoints,
            &vec![serde_json::json!(
                "https://github.com/bilalyazicioglu/Folyvo/releases/latest/download/latest.json"
            )]
        );
    }

    #[test]
    fn the_place_folyvo_runs_from_tells_whether_it_can_be_updated() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Folyvo.app/Contents/MacOS/folyvo-desktop");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        assert_eq!(Placement::of(&exe), Placement::Updatable);
        assert_eq!(Placement::of(Path::new("/repo/target/debug/folyvo-desktop")), Placement::Development);
        let translocated = Path::new(
            "/private/var/folders/x/T/AppTranslocation/AB12/d/Folyvo.app/Contents/MacOS/folyvo-desktop",
        );
        assert_eq!(Placement::of(translocated), Placement::NeedsMove);
        let missing = Path::new("/nowhere/at/all/Folyvo.app/Contents/MacOS/folyvo-desktop");
        assert_eq!(Placement::of(missing), Placement::NeedsMove, "where nothing can be written");
    }
}
