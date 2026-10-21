mod strata;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use folyvo_core::ingest::IngestOutcome;
use folyvo_core::ports::{Clock, RandomIdGen, SystemClock};
use folyvo_core::{CoreError, Vault};

fn vault_dir() -> PathBuf {
    if let Some(v) = std::env::var_os("FOLYVO_VAULT") {
        return PathBuf::from(v);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".folyvo")
}

fn run(args: &[String]) -> Result<String, String> {
    let (cmd, file) = match args {
        [cmd, file] => (cmd.as_str(), PathBuf::from(file)),
        _ => return Err("usage: folyvo <save|log> <file>".into()),
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let mut vault =
        Vault::open(&vault_dir(), clock.clone(), Arc::new(RandomIdGen)).map_err(|e| e.to_string())?;
    match cmd {
        "save" => match vault.save(&file).map_err(|e| e.to_string())? {
            IngestOutcome::Created(v) => Ok(format!("Saved v{}\n", v.seq)),
            IngestOutcome::Unchanged(v) => Ok(format!("Nothing changed since v{}\n", v.seq)),
        },
        "log" => match vault.history(&file) {
            Ok(h) => Ok(strata::render(&h, clock.now_unix_ms())),
            Err(CoreError::ItemMissing) => Err(format!("{} has no saved versions yet", file.display())),
            Err(e) => Err(e.to_string()),
        },
        other => Err(format!("unknown command {other:?}; use save or log")),
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
