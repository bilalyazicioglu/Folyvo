use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use folyvo_core::ports::{RandomIdGen, SystemClock};
use folyvo_desktop::dto::{EngineEventDto, StatusDto};
use folyvo_desktop::runtime::{AppRuntime, Notifier};

struct Silent;
impl Notifier for Silent {
    fn notify(&self, _event: EngineEventDto) {}
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn the_whole_loop_works_through_the_app_api() {
    let dir = tempfile::tempdir().unwrap();
    let (folder, out) = (dir.path().join("Client work"), dir.path().join("sent"));
    fs::create_dir(&folder).unwrap();
    fs::create_dir(&out).unwrap();
    let rt = AppRuntime::start(
        &dir.path().join("vault"),
        Arc::new(SystemClock),
        Arc::new(RandomIdGen),
        (100, 100),
        Arc::new(Silent),
    )
    .unwrap();
    let api = rt.api();

    // 1. Add a folder, save a file: a layer appears.
    rt.add_folder(&folder).unwrap();
    let file = folder.join("Proposal.docx");
    fs::write(&file, b"first draft").unwrap();
    wait_until("v1", || api.recent(5).unwrap().first().map(|i| i.latest_seq) == Some(1));
    let item = api.recent(5).unwrap()[0].item_id.clone();

    // 2. Mark it ready and export it.
    let v1 = api.history(&item).unwrap()[0].id.clone();
    api.mark_ready(&item, &v1).unwrap();
    let exported = api.export(&item, &v1, out.to_str().unwrap()).unwrap();
    assert_eq!(fs::read(&exported.destination).unwrap(), b"first draft");

    // 3. Keep working: the item reads "changed since ready".
    fs::write(&file, b"second draft, with pricing").unwrap();
    wait_until("v2", || api.recent(5).unwrap()[0].latest_seq == 2);
    assert_eq!(api.recent(5).unwrap()[0].status, StatusDto::ChangedSinceReady);

    // 4. Restore the version that was sent: a new layer, the file is back as it was sent.
    let restored = api.restore(&item, &v1).unwrap();
    assert_eq!(restored.seq, 3);
    assert_eq!(fs::read(&file).unwrap(), b"first draft");
    assert_eq!(api.recent(5).unwrap()[0].status, StatusDto::Ready);
    assert_eq!(api.history(&item).unwrap().len(), 3, "nothing was lost");
}
