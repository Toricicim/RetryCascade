use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use retrycascade_core::config::load_resolved_run_config;
use retrycascade_core::simulation::run_simulation;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("retrycascade: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: retrycascade-cli <resolved_run_config.json> [run_summary.json]")?;
    let output = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("usage: retrycascade-cli <resolved_run_config.json> [run_summary.json]".into());
    }

    let config = load_resolved_run_config(input)?;
    let summary = run_simulation(&config)?;
    let json = serde_json::to_string_pretty(&summary)?;

    if let Some(path) = output {
        fs::write(path, format!("{json}\n"))?;
    } else {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{json}")?;
    }
    Ok(())
}
