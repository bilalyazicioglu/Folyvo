mod strata;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use folyvo_core::ingest::IngestOutcome;
use folyvo_core::ports::{Clock, RandomIdGen, SystemClock};
use folyvo_core::{CoreError, Vault};

const USAGE: &str =
    "usage: folyvo save <file> | log <file> | export <file> <folder> | restore <file> <vN> | watch <folder>";

fn vault_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("FOLYVO_VAULT") {
        return PathBuf::from(v);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".folyvo")
}

fn version_number(arg: &str) -> Result<i64, String> {
    arg.strip_prefix('v')
        .unwrap_or(arg)
        .parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{arg:?} is not a version number; use a version like v2"))
}

fn run(args: &[String]) -> Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let open = || Vault::open(&vault_dir(), clock.clone(), Arc::new(RandomIdGen)).map_err(|e| e.to_string());
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
            Ok(strata::render(&h, clock.now_unix_ms()))
        }
        ["export", file, folder] => {
            let mut vault = open()?;
            let path = PathBuf::from(file);
            let seq = vault.history(&path).ok().and_then(|h| h.versions.first().map(|v| v.seq));
            let r = vault.export(&path, &PathBuf::from(folder)).map_err(|e| plain(e, file))?;
            Ok(format!("Exported v{} to {}\n", seq.unwrap_or_default(), r.destination.display()))
        }
        ["restore", file, v] => {
            let n = version_number(v)?;
            let new = open()?.restore(&PathBuf::from(file), n).map_err(|e| plain(e, file))?;
            Ok(format!("Restored v{n} as v{}\n", new.seq))
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
