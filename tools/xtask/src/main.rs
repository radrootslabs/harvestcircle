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
