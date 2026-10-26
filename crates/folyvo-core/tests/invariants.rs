use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use folyvo_core::engine::Engine;
use folyvo_core::model::{ContentHash, ItemId, VersionId};
use folyvo_core::ports::{FakeClock, SeqIdGen};
use proptest::prelude::*;

const FILES: usize = 2;
const CONTENTS: [&[u8]; 4] = [b"", b"alpha", b"beta beta", b"gamma gamma gamma"];
const SERVICES: [&str; 5] = ["mail", "airdrop", "messages", "copy", "other:Blip"];

#[derive(Debug, Clone)]
enum Op {
    Write { file: usize, content: usize },
    Ingest { file: usize },
    Copy { file: usize, nth: usize },
    Hand { file: usize, nth: usize, service: usize },
    Restore { file: usize, nth: usize },
    Delete { file: usize },
}

fn op() -> impl Strategy<Value = Op> {
    let f = 0..FILES;
    prop_oneof![
        (f.clone(), 0..CONTENTS.len()).prop_map(|(file, content)| Op::Write { file, content }),
        f.clone().prop_map(|file| Op::Ingest { file }),
        (f.clone(), 0usize..8).prop_map(|(file, nth)| Op::Copy { file, nth }),
        (f.clone(), 0usize..8, 0..SERVICES.len()).prop_map(|(file, nth, service)| Op::Hand {
            file,
            nth,
            service
        }),
        (f.clone(), 0usize..8).prop_map(|(file, nth)| Op::Restore { file, nth }),
        f.prop_map(|file| Op::Delete { file }),
    ]
}

fn hash_file(p: &Path) -> ContentHash {
    ContentHash::of_bytes(&fs::read(p).unwrap())
}

struct World {
    dir: tempfile::TempDir,
    engine: Engine,
    clock: Arc<FakeClock>,
    item: [Option<ItemId>; FILES],
    exists: [bool; FILES],
    seen: HashMap<ItemId, HashSet<VersionId>>,
    /// Every materialised file with the version it holds.
    materialised: Vec<(PathBuf, ContentHash, VersionId)>,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        fs::create_dir(dir.path().join("out")).unwrap();
        let clock = Arc::new(FakeClock::new(1_000));
        let engine =
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        World {
            dir,
            engine,
            clock,
            item: [None, None],
            exists: [false; FILES],
            seen: HashMap::new(),
            materialised: vec![],
        }
    }

    fn path(&self, file: usize) -> PathBuf {
        self.dir.path().join("work").join(format!("file{file}.docx"))
    }

    fn hash_of(&self, item: &ItemId, version: &VersionId) -> ContentHash {
        self.engine
            .history(item)
            .unwrap()
            .into_iter()
            .find(|v| &v.id == version)
            .expect("version exists")
            .content_hash
    }

    fn version_counts(&self) -> Vec<usize> {
        self.item.iter().flatten().map(|i| self.engine.history(i).unwrap().len()).collect()
    }

    fn apply(&mut self, op: &Op) {
        self.clock.advance(1_000);
        match *op {
            Op::Write { file, content } => {
                fs::write(self.path(file), CONTENTS[content]).unwrap();
                self.exists[file] = true;
            }
            Op::Ingest { file } if self.exists[file] => {
                let out = self.engine.ingest_path(&self.path(file)).unwrap().unwrap();
                self.item[file] = Some(out.version().item_id.clone());
            }
            Op::Delete { file } if self.exists[file] => {
                fs::remove_file(self.path(file)).unwrap();
                self.exists[file] = false;
                self.engine.mark_missing(&self.path(file)).unwrap();
            }
            Op::Copy { file, nth } => {
                let Some(item) = self.item[file].clone() else { return };
                let history = self.engine.history(&item).unwrap();
                let chosen = history[nth % history.len()].id.clone();
                let r = self.engine.export(&item, &chosen, &self.dir.path().join("out")).unwrap();
                let chosen_hash = self.hash_of(&item, &chosen);
                assert_eq!(
                    hash_file(&r.destination),
                    chosen_hash,
                    "a copy carries the chosen version's bytes"
                );
                self.materialised.push((r.materialized, chosen_hash, chosen));
            }
            Op::Hand { file, nth, service } => {
                let Some(item) = self.item[file].clone() else { return };
                let history = self.engine.history(&item).unwrap();
                let chosen = history[nth % history.len()].id.clone();
                let counts_before = self.version_counts();
                let path = self.engine.materialise(&item, &chosen).unwrap();
                self.engine.record_handed(&item, &chosen, SERVICES[service]).unwrap();
                assert_eq!(
                    self.version_counts(),
                    counts_before,
                    "handing a copy out never changes any history"
                );
                let chosen_hash = self.hash_of(&item, &chosen);
                self.materialised.push((path, chosen_hash, chosen));
            }
            Op::Restore { file, nth } => {
                let Some(item) = self.item[file].clone() else { return };
                let history = self.engine.history(&item).unwrap();
                let target = &history[nth % history.len()];
                let same_content_as_latest = target.content_hash == history[0].content_hash;
                let before = history.len();
                let target_hash = target.content_hash.clone();
                self.engine.restore(&item, &target.id).unwrap();
                self.exists[file] = true;
                assert_eq!(
                    hash_file(&self.path(file)),
                    target_hash,
                    "a restore leaves exactly the chosen version's bytes in the working file"
                );
                let after = self.engine.history(&item).unwrap().len();
                if same_content_as_latest {
                    assert!(after >= before, "history never shrinks");
                } else {
                    assert!(after > before, "restoring different content adds a version");
                }
            }
            _ => {}
        }
    }

    fn check(&mut self) {
        for item in self.item.iter().flatten() {
            let history = self.engine.history(item).unwrap();
            let seqs: Vec<i64> = history.iter().map(|v| v.seq).collect();
            let expected: Vec<i64> = (1..=history.len() as i64).rev().collect();
            assert_eq!(seqs, expected, "sequence numbers are contiguous");

            let ids: HashSet<VersionId> = history.iter().map(|v| v.id.clone()).collect();
            let seen = self.seen.entry(item.clone()).or_default();
            assert!(seen.iter().all(|old| ids.contains(old)), "a version never disappears");
            seen.extend(ids);

            for v in &history {
                let object = self.engine.object_path(&v.content_hash);
                assert_eq!(hash_file(&object), v.content_hash, "stored bytes always match the recorded hash");
            }
        }
        for (path, hash, version) in &self.materialised {
            assert_eq!(&hash_file(path), hash, "a materialised copy never changes: {path:?}");
            let found = self.engine.which_one(path).unwrap();
            assert!(
                found.iter().any(|m| &m.version.id == version),
                "which-one recognises every copy: {path:?}"
            );
        }
    }
}

fn config() -> ProptestConfig {
    let mut c = ProptestConfig::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        c.cases = 32;
    }
    c
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn the_vault_keeps_its_promises_under_random_use(ops in proptest::collection::vec(op(), 1..40)) {
        let mut world = World::new();
        for step in &ops {
            world.apply(step);
            world.check();
        }
    }
}
