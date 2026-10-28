//! How much reading a file may cost. A hostile or huge document runs out of budget and is
//! `Unreadable`, never a crash or a hang.

use std::time::{Duration, Instant};

use crate::Unreadable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub file_bytes: u64,
    pub zip_entries: usize,
    /// One part of a zip, uncompressed.
    pub part_bytes: u64,
    /// Uncompressed size over compressed size, for one part.
    pub ratio: u64,
    /// All the text read out of one file.
    pub text_bytes: usize,
    pub cells: usize,
    pub time: Duration,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        file_bytes: 50 * 1024 * 1024,
        zip_entries: 10_000,
        part_bytes: 32 * 1024 * 1024,
        ratio: 100,
        text_bytes: 4 * 1024 * 1024,
        cells: 200_000,
        time: Duration::from_secs(2),
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

/// What is left of the limits while one file is read.
pub(crate) struct Budget {
    pub(crate) limits: Limits,
    deadline: Instant,
    text: usize,
}

impl Budget {
    pub(crate) fn new(limits: &Limits) -> Budget {
        Budget { limits: limits.clone(), deadline: Instant::now() + limits.time, text: 0 }
    }

    pub(crate) fn tick(&mut self) -> Result<(), Unreadable> {
        if Instant::now() >= self.deadline {
            Err(Unreadable::TooSlow)
        } else {
            Ok(())
        }
    }

    pub(crate) fn add_text(&mut self, n: usize) -> Result<(), Unreadable> {
        self.text += n;
        if self.text > self.limits.text_bytes {
            Err(Unreadable::TooLarge)
        } else {
            Ok(())
        }
    }
}
