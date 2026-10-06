use harvestcircle_xtask::{Command, run};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let Some(command) = arguments.next() else {
        eprintln!(
            "usage: cargo run --manifest-path tools/xtask/Cargo.toml -- <design-source-audit|repo-audit|namespace-audit|provenance-check|qualification-report>"
        );
        return ExitCode::FAILURE;
    };
    if command == "affected-report" {
        let mut base = env::var("HARVESTCIRCLE_AFFECTED_BASE")
            .ok()
            .filter(|v| !v.is_empty());
        let mut head = "HEAD".to_owned();
        let mut seen_head = false;
        let mut seen_base = false;
        while let Some(flag) = arguments.next() {
            let Some(value) = arguments.next() else {
                eprintln!("affected-report requires --base <commit> and optional --head <commit>");
                return ExitCode::FAILURE;
            };
            match flag.as_str() {
                "--base" if !seen_base => {
                    base = Some(value);
                    seen_base = true;
                }
                "--head" if !seen_head => {
                    head = value;
                    seen_head = true;
                }
                _ => {
                    eprintln!("unknown or repeated affected-report option");
                    return ExitCode::FAILURE;
                }
            }
        }
        let result = env::current_dir()
            .map_err(|e| e.to_string())
            .and_then(|root| harvestcircle_xtask::affected::report(&root, base.as_deref(), &head));
        return match result {
            Ok((report, green)) => {
                print!("{report}");
                if green {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        };
    }
    if arguments.next().is_some() {
        eprintln!("xtask commands do not accept positional arguments");
        return ExitCode::FAILURE;
    }
    let command = match command.parse::<Command>() {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let root = match env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("unable to resolve the HarvestCircle repository root: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run(&root, command) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(findings) => {
            for finding in findings {
                eprintln!("{finding}");
            }
            ExitCode::FAILURE
        }
    }
}
