mod log;
mod watch;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use chrono::{Local, TimeZone};
use folyvo_core::ingest::IngestOutcome;
use folyvo_core::model::VersionId;
use folyvo_core::ports::{RandomIdGen, SystemClock};
use folyvo_core::{CoreError, Vault};

const USAGE: &str = "usage: folyvo save <file> | log <file> | copy <file> <folder> [version] | which <file> | restore <file> <version> | watch <folder>";

const NO_MATCH: &str =
    "No saved version has exactly this content. It may have been opened and saved again since it left.";

/// The same vault the desktop app uses, unless `FOLYVO_VAULT` says otherwise.
fn vault_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("FOLYVO_VAULT") {
        return PathBuf::from(v);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("Folyvo")
}

/// Local time, minute precision.
fn when(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

fn code_of(vault: &Vault, path: &Path, id: &VersionId) -> String {
    vault
        .history(path)
        .ok()
        .and_then(|h| h.versions.iter().position(|v| &v.id == id).map(|i| h.codes[i].clone()))
        .unwrap_or_default()
}

fn run(args: &[String]) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let open =
        || Vault::open(&vault_dir(), Arc::new(SystemClock), Arc::new(RandomIdGen)).map_err(|e| e.to_string());
    let plain = |e: CoreError, file: &str| match e {
        CoreError::ItemMissing => format!("{file} has no saved versions yet"),
        other => other.to_string(),
    };
    match args.as_slice() {
        ["save", file] => match open()?.save(&PathBuf::from(file)).map_err(|e| e.to_string())? {
            IngestOutcome::Created(v) => Ok(format!("Saved v{}\n", v.seq)),
            IngestOutcome::Unchanged(v) => Ok(format!("Nothing changed since v{}\n", v.seq)),
        },
        ["log", file] => {
            let h = open()?.history(&PathBuf::from(file)).map_err(|e| plain(e, file))?;
            Ok(log::render(&h, &when))
        }
        ["copy", file, folder, rest @ ..] if rest.len() <= 1 => {
            let mut vault = open()?;
            let path = PathBuf::from(file);
            let r = vault
                .copy(&path, rest.first().copied(), &PathBuf::from(folder))
                .map_err(|e| plain(e, file))?;
            Ok(format!(
                "Saved a copy of {} to {}\n",
                code_of(&vault, &path, &r.version_id),
                r.destination.display()
            ))
        }
        ["which", file] => {
            let matches = open()?.which(&PathBuf::from(file)).map_err(|e| e.to_string())?;
            if matches.is_empty() {
                return Ok(format!("{NO_MATCH}\n"));
            }
            let mut out = String::new();
            for m in matches {
                out.push_str(&format!("{}, {} ({})\n", m.name, when(m.version.created_at), m.version.code));
                for e in &m.events {
                    out.push_str(&format!("  {}\n", log::event_line(e, &when)));
                }
            }
            Ok(out)
        }
        ["restore", file, v] => {
            let new = open()?.restore(&PathBuf::from(file), v).map_err(|e| plain(e, file))?;
            Ok(format!("Restored {v} as v{}\n", new.seq))
        }
        ["watch", folder] => {
            watch::run(&vault_dir(), &PathBuf::from(folder))?;
            Ok(String::new())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
