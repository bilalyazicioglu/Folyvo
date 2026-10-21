use std::path::Path;
use std::sync::Arc;

use folyvo_core::debounce::{KNOWN_QUIET_MS, NEW_QUIET_MS};
use folyvo_core::engine::{Engine, EngineEvent};
use folyvo_core::ports::{Clock, SystemClock};
use folyvo_core::runner::{OnApplied, Runner};
use folyvo_core::watch::Applied;

/// `FOLYVO_QUIET_MS="known,new"` shortens the quiet periods; tests use it, users never need to.
fn quiet_periods() -> (i64, i64) {
    std::env::var("FOLYVO_QUIET_MS")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .unwrap_or((KNOWN_QUIET_MS, NEW_QUIET_MS))
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

pub fn describe(applied: &Applied) -> Option<String> {
    match applied {
        Applied::Saved { path, version } => Some(format!("Saved {} as v{}", name_of(path), version.seq)),
        Applied::Missing { path } => Some(format!("{} is missing", name_of(path))),
        Applied::Failed { path, reason } => Some(format!("Could not save {}: {reason}", name_of(path))),
        Applied::Unchanged { .. } | Applied::Skipped { .. } => None,
    }
}

fn announce_startup_scan(engine: &Engine, events: &mut folyvo_core::engine::EventReceiver) {
    let names: Vec<(folyvo_core::model::ItemId, String)> =
        engine.list_recent(u32::MAX).unwrap_or_default().into_iter().map(|s| (s.item_id, s.name)).collect();
    while let Ok(event) = events.try_recv() {
        if let EngineEvent::VersionCreated { item, version } = event {
            let seq = engine
                .history(&item)
                .ok()
                .and_then(|h| h.into_iter().find(|v| v.id == version).map(|v| v.seq))
                .unwrap_or_default();
            let name = names.iter().find(|(id, _)| *id == item).map(|(_, n)| n.as_str()).unwrap_or("a file");
            println!("Saved {name} as v{seq}");
        }
    }
}

pub fn run(vault: &Path, folder: &Path) -> Result<(), String> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let engine = Arc::new(
        Engine::open(vault, clock.clone(), Arc::new(folyvo_core::ports::RandomIdGen))
            .map_err(|e| e.to_string())?,
    );
    let cb: OnApplied = Arc::new(|applied| {
        if let Some(line) = describe(applied) {
            println!("{line}");
        }
    });
    let mut events = engine.subscribe();
    let (runner, report) =
        Runner::start(engine.clone(), clock, folder, quiet_periods(), cb).map_err(|e| e.to_string())?;
    announce_startup_scan(&engine, &mut events);
    for (path, why) in &report.failed {
        println!("Could not read {}: {why}", path.display());
    }
    drop(events);
    println!("Watching {}", runner.folder().display());
    loop {
        std::thread::park(); // the runner thread does the work; Ctrl-C ends the process
    }
}
