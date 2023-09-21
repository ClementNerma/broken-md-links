#![forbid(unsafe_code)]
#![forbid(unused_must_use)]

use anyhow::{bail, Result};
use broken_md_links::{check_broken_links, CheckerError, CheckerOptions, DetectedBrokenLink};
use clap::Parser;
use colored::Colorize;
use log::{error, LevelFilter};
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

/// Command
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Command {
    #[clap(help = "Input file or directory")]
    pub input: String,

    #[clap(
        long,
        help = "Do not check if headers are valids in links (e.g. 'document.md#some-header')"
    )]
    pub ignore_header_links: bool,

    #[clap(short, long, default_value = "warn", help = "Verbosity level")]
    pub verbosity: LevelFilter,

    #[clap(long, help = "Only accept links to files")]
    pub disallow_dir_links: bool,
}

/// Command-line entrypoint
fn main() -> ExitCode {
    match inner_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err:?}");
            ExitCode::FAILURE
        }
    }
}

fn inner_main() -> Result<()> {
    let Command {
        input,
        ignore_header_links,
        verbosity,
        disallow_dir_links,
    } = Command::parse();

    // Initialize the logger
    env_logger::builder().filter_level(verbosity).init();

    let input = Path::new(&input);

    if !input.exists() {
        bail!("Input path not found");
    }

    match check_broken_links(
        input,
        CheckerOptions {
            ignore_header_links,
            disallow_dir_links,
        },
        &mut HashMap::new(),
    ) {
        Ok(()) => Ok(()),
        Err(err) => match err {
            CheckerError::Io(err) => bail!("IO error: {err}"),
            CheckerError::BrokenLinks(err) => bail!(
                "Detected {} broken link{}:{}",
                err.len(),
                if err.len() > 1 { "s" } else { "" },
                err.into_iter()
                    .map(|DetectedBrokenLink { file, line, error }| format!(
                        "\n* In {}:{}: {}",
                        file.to_string_lossy().bright_magenta(),
                        line.to_string().bright_cyan(),
                        error.bright_yellow()
                    ))
                    .collect::<String>()
            ),
        },
    }
}
