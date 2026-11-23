//! F7 CLI: thin wrapper over headless + GamedevContext.
//!
//! ```text
//! dither-yuki apply [--pattern X] [--project P] [--context C.json]
//!                   [--cell WxH] [--tile] [--png8] [--lock-phase]
//!                   [--name-template T] <in> <out>
//! ```

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use engine_project::{parse_cell_wh, run_cli_apply, CliApplyOptions};

fn main() -> ExitCode {
    let mut args = env::args().skip(1).peekable();
    let cmd = args.next().unwrap_or_default();
    match cmd.as_str() {
        "apply" => match apply(args) {
            Ok(n) => {
                println!("ok files={n}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        },
        "version" | "--version" | "-V" => {
            println!("dither-yuki {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" | "" => {
            print_usage();
            ExitCode::from(if cmd.is_empty() { 2 } else { 0 })
        }
        other => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: dither-yuki apply [--pattern X.dyuki] [--project P.dyproj] [--context C.json]\n\
                          [--cell WxH] [--tile] [--png8] [--lock-phase]\n\
                          [--name-template T] <in> <out>\n\
               dither-yuki version"
    );
}

fn apply(mut args: impl Iterator<Item = String>) -> Result<usize, String> {
    let mut opts = CliApplyOptions::default();
    let mut positional = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pattern" => {
                opts.pattern = Some(PathBuf::from(
                    args.next().ok_or("--pattern requires a path")?,
                ));
            }
            "--project" => {
                opts.project = Some(PathBuf::from(
                    args.next().ok_or("--project requires a path")?,
                ));
            }
            "--context" => {
                opts.context_json = Some(PathBuf::from(
                    args.next().ok_or("--context requires a path")?,
                ));
            }
            "--cell" => {
                let s = args.next().ok_or("--cell requires WxH")?;
                opts.cell = Some(parse_cell_wh(&s).map_err(|e| e.to_string())?);
            }
            "--tile" => opts.tile = true,
            "--png8" => opts.png8 = true,
            "--lock-phase" => opts.lock_pattern_phase = true,
            "--no-lock-phase" => opts.lock_pattern_phase = false,
            "--name-template" => {
                opts.name_template = args.next().ok_or("--name-template requires a value")?;
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(0);
            }
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            s => positional.push(PathBuf::from(s)),
        }
    }

    if positional.len() != 2 {
        return Err("apply requires <in> <out>".into());
    }
    let input = &positional[0];
    let output = &positional[1];
    let results = run_cli_apply(&opts, input, output).map_err(|e| e.to_string())?;
    let ok = results.iter().filter(|r| r.error.is_none()).count();
    let fail = results.len().saturating_sub(ok);
    if fail > 0 {
        for r in &results {
            if let Some(err) = &r.error {
                eprintln!("{}: {err}", r.input.display());
            }
        }
        return Err(format!("{fail} file(s) failed"));
    }
    Ok(ok)
}
