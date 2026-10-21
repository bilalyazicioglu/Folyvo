use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Clock: Send + Sync {
    fn now_unix_ms(&self) -> i64;
}

pub trait IdGen: Send + Sync {
    /// Lowercase hex suffix for a new id (the caller adds the `itm_` / `ver_` prefix).
    fn next_suffix(&self) -> String;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now_unix_ms(&self) -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

pub struct RandomIdGen;
impl IdGen for RandomIdGen {
    fn next_suffix(&self) -> String {
        uuid::Uuid::new_v4().simple().to_string()
    }
}

/// Test double: time moves only when told to.
pub struct FakeClock(AtomicI64);
impl FakeClock {
    pub fn new(start_ms: i64) -> Self {
        Self(AtomicI64::new(start_ms))
    }
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}
impl Clock for FakeClock {
    fn now_unix_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Test double: deterministic ids `00000001`, `00000002`, …
pub struct SeqIdGen(AtomicU64);
impl SeqIdGen {
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }
}
impl Default for SeqIdGen {
    fn default() -> Self {
        Self::new()
    }
}
impl IdGen for SeqIdGen {
    fn next_suffix(&self) -> String {
        format!("{:08x}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_only_moves_when_told() {
        let c = FakeClock::new(1_000);
        assert_eq!(c.now_unix_ms(), 1_000);
        c.advance(250);
        assert_eq!(c.now_unix_ms(), 1_250);
        c.set(5);
        assert_eq!(c.now_unix_ms(), 5);
    }

    #[test]
    fn seq_id_gen_is_deterministic_lowercase_hex() {
        let g = SeqIdGen::new();
        assert_eq!(g.next_suffix(), "00000001");
        assert_eq!(g.next_suffix(), "00000002");
    }

    #[test]
    fn random_id_gen_yields_32_distinct_lowercase_hex_chars() {
        let g = RandomIdGen;
        let (a, b) = (g.next_suffix(), g.next_suffix());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        assert_ne!(a, b);
    }

    #[test]
    fn system_clock_is_after_2025() {
        assert!(SystemClock.now_unix_ms() > 1_735_689_600_000);
    }
}
