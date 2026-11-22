//! Letting go of older saves (#54): every save from the last 14 days, then one a day.

use std::collections::HashSet;

use crate::engine::{Engine, EngineEvent};
use crate::error::{CoreError, Result};
use crate::model::{ItemId, VersionId};
use crate::store::repo;

/// How long every save of a file is kept.
pub const EVERY_SAVE_FOR_MS: i64 = 14 * 24 * 60 * 60 * 1_000;

/// Whether older saves are let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Not shown to the person yet: nothing goes until it has been.
    Unasked,
    On,
    /// "Keep Every Save".
    Off,
}

/// What letting go took, or would take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LetGo {
    pub saves: usize,
    pub bytes: u64,
}

/// The last time older saves were let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastLetGo {
    pub at: i64,
    pub saves: usize,
    pub bytes: u64,
}

const RULE: &str = "letting_go";
const LAST: &str = "last_let_go";
/// How many saves go between two looks at the vault by everything else.
const BATCH: usize = 200;

impl Engine {
    pub fn letting_go(&self) -> Result<Rule> {
        Ok(match repo::setting(self.lock().store.conn(), RULE)?.as_deref() {
            Some("on") => Rule::On,
            Some("off") => Rule::Off,
            _ => Rule::Unasked,
        })
    }

    pub fn set_letting_go(&self, on: bool) -> Result<()> {
        repo::set_setting(self.lock().store.conn(), RULE, if on { "on" } else { "off" })
    }

    /// A save the person wants kept whatever the rule says, or no longer.
    pub fn keep_save(&self, version: &VersionId, keep: bool) -> Result<()> {
        if !repo::set_keep(self.lock().store.conn(), version, keep)? {
            return Err(CoreError::VersionMissing(version.clone()));
        }
        Ok(())
    }

    /// Every save of a file kept whatever the rule says, or no longer.
    pub fn keep_every_save(&self, item: &ItemId, keep: bool) -> Result<()> {
        if !repo::set_keep_all(self.lock().store.conn(), item, keep)? {
            return Err(CoreError::ItemMissing);
        }
        Ok(())
    }

    /// What the rule would let go now, whether or not it is on. The bytes count each save in full,
    /// so a stored file two saves share is counted for both: at most what would be freed.
    pub fn older_saves(&self) -> Result<LetGo> {
        let g = self.lock();
        let c = g.store.conn();
        let gone = self.saves_to_let_go(c)?;
        let mut bytes = 0;
        for (_, v) in &gone {
            bytes += repo::version(c, v)?.map_or(0, |v| v.size);
        }
        Ok(LetGo { saves: gone.len(), bytes })
    }

    /// Lets go of what the rule says, if it is on, and their space with them.
    pub fn let_go_of_older_saves(&self) -> Result<LetGo> {
        self.let_go_in_batches_of(BATCH)
    }

    /// A few saves at a time, letting go of the vault in between so saves go on being kept.
    pub(crate) fn let_go_in_batches_of(&self, batch: usize) -> Result<LetGo> {
        if self.letting_go()? != Rule::On {
            return Ok(LetGo::default());
        }
        let mut done = LetGo::default();
        loop {
            let step = self.let_go_some(batch)?;
            if step.saves == 0 {
                break;
            }
            done.saves += step.saves;
            done.bytes += step.bytes;
        }
        if done.saves > 0 {
            let at = self.clock.now_unix_ms();
            repo::set_setting(
                self.lock().store.conn(),
                LAST,
                &format!("{at} {} {}", done.saves, done.bytes),
            )?;
        }
        Ok(done)
    }

    fn let_go_some(&self, batch: usize) -> Result<LetGo> {
        let mut g = self.lock();
        let inner = &mut *g;
        // Writing from the start: no other process can mark, note or send a save between it
        // being chosen and let go.
        let tx =
            inner.store.conn_mut().transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut gone = self.saves_to_let_go(&tx)?;
        gone.truncate(batch);
        let ids: Vec<VersionId> = gone.iter().map(|(_, v)| v.clone()).collect();
        let deleted = repo::delete_versions(&tx, &ids)?;
        if deleted.versions.is_empty() {
            return Ok(LetGo::default());
        }
        tx.commit()?;
        // A file left behind by a crash here has no row, and goes when the vault next opens.
        for (hash, _) in &deleted.unused {
            inner.cas.remove_object(hash)?;
        }
        drop(g);
        let items: HashSet<ItemId> =
            gone.into_iter().filter(|(_, v)| deleted.versions.contains(v)).map(|(item, _)| item).collect();
        for item in items {
            self.emit(EngineEvent::ItemUpdated { item });
        }
        Ok(LetGo { saves: deleted.versions.len(), bytes: deleted.unused.iter().map(|(_, size)| size).sum() })
    }

    pub fn last_let_go(&self) -> Result<Option<LastLetGo>> {
        let text = repo::setting(self.lock().store.conn(), LAST)?;
        Ok(text.and_then(|t| {
            let mut parts = t.split(' ').map(str::parse::<i64>);
            match (parts.next()?, parts.next()?, parts.next()?) {
                (Ok(at), Ok(saves), Ok(bytes)) => {
                    Some(LastLetGo { at, saves: saves as usize, bytes: bytes as u64 })
                }
                _ => None,
            }
        }))
    }

    fn saves_to_let_go(&self, c: &rusqlite::Connection) -> Result<Vec<(ItemId, VersionId)>> {
        let now = self.clock.now_unix_ms();
        let mut out = Vec::new();
        let saves = repo::saves_for_letting_go(c)?;
        for chunk in saves.chunk_by(|a, b| a.0 == b.0) {
            let item = &chunk[0].0;
            let file: Vec<Save> = chunk.iter().map(|(_, s)| s.clone()).collect();
            out.extend(to_let_go(&file, now, &local_day).into_iter().map(|v| (item.clone(), v)));
        }
        Ok(out)
    }
}

/// The day a moment falls on where this Mac is.
fn local_day(at: i64) -> i64 {
    jiff::Timestamp::from_millisecond(at).map_or(at.div_euclid(24 * 60 * 60 * 1_000), |t| {
        let date = t.to_zoned_datetime(jiff::tz::TimeZone::system()).date();
        i64::from(date.year()) * 10_000 + i64::from(date.month()) * 100 + i64::from(date.day())
    })
}

/// One save of a file, as the rule sees it.
#[derive(Debug, Clone)]
pub struct Save {
    pub id: VersionId,
    pub at: i64,
    /// Marked, sent, written a note on, or of a file whose every save is kept.
    pub protected: bool,
}

/// The saves of one file, oldest first, that the rule lets go at `now`. `day_of` tells which
/// day a moment falls on, where the person is.
pub fn to_let_go(saves: &[Save], now: i64, day_of: &dyn Fn(i64) -> i64) -> Vec<VersionId> {
    let last = saves.len().saturating_sub(1);
    saves
        .iter()
        .enumerate()
        .filter(|&(i, s)| {
            let last_of_its_day = saves.get(i + 1).is_none_or(|next| day_of(next.at) != day_of(s.at));
            i != 0 && i != last && !s.protected && s.at < now - EVERY_SAVE_FOR_MS && !last_of_its_day
        })
        .map(|(_, s)| s.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1_000;
    const NOW: i64 = 100 * DAY;

    fn day_of(at: i64) -> i64 {
        at.div_euclid(DAY)
    }

    fn save(n: u32, at: i64) -> Save {
        Save { id: VersionId::parse(&format!("ver_{n:04}")).unwrap(), at, protected: false }
    }

    fn gone(saves: &[Save]) -> Vec<String> {
        to_let_go(saves, NOW, &day_of).iter().map(|v| v.as_str().to_string()).collect()
    }

    #[test]
    fn every_save_of_the_last_fourteen_days_stays() {
        let saves: Vec<_> = (0..10).map(|n| save(n, NOW - 13 * DAY + i64::from(n) * 1_000)).collect();
        assert!(gone(&saves).is_empty());
    }

    #[test]
    fn before_that_the_last_save_of_each_day_stays() {
        let old = NOW - 20 * DAY;
        let saves = vec![
            save(1, old),               // first save of the file: stays
            save(2, old + 1_000),       // gone
            save(3, old + 2_000),       // last of its day: stays
            save(4, old + DAY + 1_000), // gone
            save(5, old + DAY + 2_000), // last of its day: stays
            save(6, NOW - DAY),         // recent: stays
        ];
        assert_eq!(gone(&saves), vec!["ver_0002", "ver_0004"]);
    }

    #[test]
    fn the_first_and_the_last_save_always_stay() {
        let old = NOW - 30 * DAY;
        let saves = vec![save(1, old), save(2, old + 1_000), save(3, old + 2_000)];
        assert_eq!(gone(&saves), vec!["ver_0002"]);
    }

    #[test]
    fn a_protected_save_stays_however_old() {
        let old = NOW - 30 * DAY;
        let mut saves = vec![save(1, old), save(2, old + 1_000), save(3, old + 2_000), save(4, NOW)];
        saves[1].protected = true;
        assert!(gone(&saves).is_empty());
    }

    #[test]
    fn a_save_exactly_fourteen_days_old_stays() {
        let old = NOW - 30 * DAY;
        let saves = vec![
            save(1, old),
            save(2, NOW - EVERY_SAVE_FOR_MS),
            save(3, NOW - EVERY_SAVE_FOR_MS + 1),
            save(4, NOW),
        ];
        assert!(gone(&saves).is_empty());
    }

    #[test]
    fn the_day_is_the_persons_own() {
        // Two saves an hour apart, on one day in UTC but on two days three hours east of it.
        let old = NOW - 20 * DAY + 20 * 60 * 60 * 1_000; // 20:00 UTC
        let saves = vec![save(1, old - DAY), save(2, old), save(3, old + 2 * 60 * 60 * 1_000), save(4, NOW)];
        let east = |at: i64| (at + 3 * 60 * 60 * 1_000).div_euclid(DAY);
        let in_utc: Vec<_> = to_let_go(&saves, NOW, &day_of).iter().map(|v| v.as_str().to_string()).collect();
        let in_east: Vec<_> = to_let_go(&saves, NOW, &east).iter().map(|v| v.as_str().to_string()).collect();
        assert_eq!(in_utc, vec!["ver_0002"]);
        assert!(in_east.is_empty());
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::model::Version;
    use crate::ports::{FakeClock, SeqIdGen};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const DAY: i64 = 24 * 60 * 60 * 1_000;
    /// Noon UTC: the same date anywhere from eleven hours west to eleven hours east.
    const NOON: i64 = 20_000 * DAY + DAY / 2;

    struct T {
        dir: tempfile::TempDir,
        engine: Engine,
        clock: Arc<FakeClock>,
    }

    fn setup() -> T {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("work")).unwrap();
        let clock = Arc::new(FakeClock::new(NOON));
        let engine =
            Engine::open(&dir.path().join("vault"), clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        T { dir, engine, clock }
    }

    impl T {
        fn file(&self, name: &str) -> PathBuf {
            self.dir.path().join("work").join(name)
        }
        fn save_at(&self, p: &Path, bytes: &[u8], at: i64) -> Version {
            fs::write(p, bytes).unwrap();
            self.clock.set(at);
            self.engine.ingest_path(p).unwrap().unwrap().version().clone()
        }
        /// A file saved three times on one day a month ago, then today: the middle save goes.
        fn month_old(&self, name: &str) -> Vec<Version> {
            let p = self.file(name);
            let old = NOON - 30 * DAY;
            let saves = vec![
                self.save_at(&p, format!("{name} 1").as_bytes(), old),
                self.save_at(&p, format!("{name} 2").as_bytes(), old + 1_000),
                self.save_at(&p, format!("{name} 3").as_bytes(), old + 2_000),
                self.save_at(&p, format!("{name} 4").as_bytes(), NOON),
            ];
            self.clock.set(NOON);
            saves
        }
        fn stored(&self, v: &Version) -> bool {
            crate::cas::Cas::open(&self.dir.path().join("vault")).unwrap().contains(&v.content_hash)
        }
        fn restored_from(&self, v: &VersionId) -> Option<String> {
            let c = rusqlite::Connection::open(self.dir.path().join("vault").join("folyvo.db")).unwrap();
            c.query_row("SELECT restored_from FROM versions WHERE id = ?1", [v.as_str()], |r| r.get(0))
                .unwrap()
        }
        fn kept(&self, v: &Version) -> bool {
            self.engine.history(&v.item_id).unwrap().iter().any(|h| h.id == v.id)
        }
    }

    #[test]
    fn nothing_goes_while_the_rule_has_not_been_shown() {
        let t = setup();
        let saves = t.month_old("a.docx");
        assert_eq!(t.engine.letting_go().unwrap(), Rule::Unasked);
        assert_eq!(t.engine.older_saves().unwrap().saves, 1, "what would go is known");
        assert_eq!(t.engine.let_go_of_older_saves().unwrap(), LetGo::default());
        assert!(t.kept(&saves[1]));
    }

    #[test]
    fn once_on_older_saves_go_with_their_space() {
        let t = setup();
        let saves = t.month_old("a.docx");
        t.engine.set_letting_go(true).unwrap();
        let before = t.engine.vault_stats().unwrap();
        let gone = t.engine.let_go_of_older_saves().unwrap();
        assert_eq!(gone, LetGo { saves: 1, bytes: "a.docx 2".len() as u64 });
        assert!(!t.kept(&saves[1]));
        assert!(t.kept(&saves[0]) && t.kept(&saves[2]) && t.kept(&saves[3]));
        let after = t.engine.vault_stats().unwrap();
        assert_eq!((after.versions, after.bytes), (before.versions - 1, before.bytes - gone.bytes));
        assert!(!t.stored(&saves[1]), "the stored file is gone too");
        assert_eq!(
            t.engine.last_let_go().unwrap(),
            Some(LastLetGo { at: NOON, saves: 1, bytes: gone.bytes })
        );
    }

    #[test]
    fn keep_every_save_lets_nothing_go() {
        let t = setup();
        let saves = t.month_old("a.docx");
        t.engine.set_letting_go(false).unwrap();
        assert_eq!(t.engine.letting_go().unwrap(), Rule::Off);
        assert_eq!(t.engine.let_go_of_older_saves().unwrap(), LetGo::default());
        assert!(t.kept(&saves[1]));
        assert_eq!(t.engine.last_let_go().unwrap(), None);
    }

    #[test]
    fn marked_noted_and_sent_saves_stay_and_every_save_of_a_kept_file() {
        let t = setup();
        let marked = t.month_old("marked.docx");
        let noted = t.month_old("noted.docx");
        let sent = t.month_old("sent.docx");
        let all = t.month_old("all.docx");
        let plain = t.month_old("plain.docx");
        t.engine.keep_save(&marked[1].id, true).unwrap();
        t.engine.name_version(&noted[1].id, Some("before the client call")).unwrap();
        t.engine.record_handed(&sent[1].item_id, &sent[1].id, "mail").unwrap();
        t.engine.keep_every_save(&all[1].item_id, true).unwrap();
        t.engine.set_letting_go(true).unwrap();
        assert_eq!(t.engine.let_go_of_older_saves().unwrap().saves, 1);
        assert!(t.kept(&marked[1]) && t.kept(&noted[1]) && t.kept(&sent[1]) && t.kept(&all[1]));
        assert!(!t.kept(&plain[1]));
    }

    #[test]
    fn a_mark_taken_back_lets_the_save_go() {
        let t = setup();
        let saves = t.month_old("a.docx");
        t.engine.keep_save(&saves[1].id, true).unwrap();
        t.engine.keep_save(&saves[1].id, false).unwrap();
        t.engine.set_letting_go(true).unwrap();
        assert_eq!(t.engine.let_go_of_older_saves().unwrap().saves, 1);
    }

    #[test]
    fn a_stored_file_another_save_still_uses_stays() {
        let t = setup();
        let saves = t.month_old("a.docx");
        let other = t.file("copy.docx");
        t.save_at(&other, b"a.docx 2", NOON); // the same bytes as the save that goes
        t.clock.set(NOON);
        t.engine.set_letting_go(true).unwrap();
        let gone = t.engine.let_go_of_older_saves().unwrap();
        assert_eq!(gone, LetGo { saves: 1, bytes: 0 });
        assert!(t.stored(&saves[1]));
    }

    #[test]
    fn the_save_after_one_that_went_is_compared_again() {
        let t = setup();
        let saves = t.month_old("a.docx");
        for (i, v) in saves.iter().enumerate() {
            let record = crate::model::ChangeRecord {
                compared_to: i.checked_sub(1).map(|before| saves[before].id.clone()),
                outcome: crate::model::ChangeOutcome::Unknown,
                counts: Vec::new(),
                reader: 1,
            };
            t.engine.store_change(&v.id, &record).unwrap();
        }
        t.engine.set_letting_go(true).unwrap();
        t.engine.let_go_of_older_saves().unwrap();
        assert_eq!(
            t.engine.versions_needing_changes(&saves[0].item_id, 1).unwrap(),
            vec![saves[2].id.clone()]
        );
    }

    #[test]
    fn a_save_restored_from_one_that_went_loses_only_the_link() {
        let t = setup();
        let saves = t.month_old("a.docx");
        let restored = t.engine.restore(&saves[1].item_id, &saves[1].id).unwrap();
        assert_eq!(restored.restored_from, Some(saves[1].id.clone()));
        t.engine.set_letting_go(true).unwrap();
        assert_eq!(t.engine.let_go_of_older_saves().unwrap().saves, 1);
        assert!(t.kept(&restored));
        assert_eq!(t.restored_from(&restored.id), None);
    }

    #[test]
    fn a_protection_added_by_another_process_is_honoured_at_the_moment_of_letting_go() {
        let t = setup();
        let saves = t.month_old("a.docx");
        let other = rusqlite::Connection::open(t.dir.path().join("vault").join("folyvo.db")).unwrap();
        other
            .execute(
                "UPDATE versions SET note = 'from the command line' WHERE id = ?1",
                [saves[1].id.as_str()],
            )
            .unwrap();
        let mut store = crate::store::Store::open(&t.dir.path().join("vault").join("folyvo.db")).unwrap();
        let tx = store.conn_mut().transaction().unwrap();
        let deleted = repo::delete_versions(&tx, &[saves[1].id.clone()]).unwrap();
        tx.commit().unwrap();
        assert!(deleted.versions.is_empty() && deleted.unused.is_empty());
        assert!(t.kept(&saves[1]));
    }

    #[test]
    fn the_codes_of_the_saves_that_stay_still_find_them() {
        let t = setup();
        let saves = t.month_old("a.docx");
        let before: Vec<(VersionId, String)> =
            t.engine.history(&saves[0].item_id).unwrap().into_iter().map(|v| (v.id, v.code)).collect();
        t.engine.set_letting_go(true).unwrap();
        t.engine.let_go_of_older_saves().unwrap();
        for v in t.engine.history(&saves[0].item_id).unwrap() {
            let old = &before.iter().find(|(id, _)| *id == v.id).unwrap().1;
            assert!(old.starts_with(&v.code), "{old} must still find the save its code is now {}", v.code);
        }
    }

    #[test]
    fn letting_go_a_few_at_a_time_takes_them_all_and_counts_them_together() {
        let t = setup();
        t.month_old("a.docx");
        t.month_old("b.docx");
        t.month_old("c.docx");
        t.engine.set_letting_go(true).unwrap();
        let gone = t.engine.let_go_in_batches_of(1).unwrap();
        assert_eq!(gone.saves, 3);
        assert_eq!(t.engine.older_saves().unwrap().saves, 0);
        assert_eq!(t.engine.last_let_go().unwrap().map(|l| (l.saves, l.bytes)), Some((3, gone.bytes)));
    }

    #[test]
    fn the_rule_is_remembered() {
        let t = setup();
        t.engine.set_letting_go(true).unwrap();
        let vault = t.dir.path().join("vault");
        drop(t.engine);
        let again = Engine::open(&vault, t.clock.clone(), Arc::new(SeqIdGen::new())).unwrap();
        assert_eq!(again.letting_go().unwrap(), Rule::On);
    }
}
