use broken_md_links::check_broken_links;
use clap::Clap;
use colored::Colorize;
use fern::colors::{Color, ColoredLevelConfig};
use log::{error, info, warn, Level, LevelFilter};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Command
#[derive(Clap)]
#[clap(
    version = "1.0",
    author = "Clément Nerma <clement.nerma@gmail.com>",
    about = "Detect broken links in markdown files"
)]
struct Command {
    #[clap(index = 1, about = "Input file or directory")]
    pub input: String,

    #[clap(
        short = "r",
        long = "recursive",
        about = "Check all files in the input directory"
    )]
    pub recursive: bool,

    #[clap(
        long = "ignore-header-links",
        about = "Do not check if headers are valids in links (e.g. 'document.md#some-header')"
    )]
    pub ignore_header_links: bool,

    #[clap(short = "v", long = "verbosity", possible_values=&["silent", "errors", "warn", "info", "verbose", "debug"],
           default_value="warn", about = "Verbosity level")]
    pub verbosity: String,

    #[clap(
        long = "no-error",
        about = "Convert all broken/invalid links errors to warnings"
    )]
    pub no_error: bool,
}

/// Start the logger, hiding every message whose level is under the provided one
/// Only messages with a level greater than or equal to the provided 'level' will be displayed
fn logger(level: LevelFilter) {
    // Create color scheme
    let colors_line = ColoredLevelConfig::new()
        .error(Color::Red)
        .warn(Color::Yellow)
        .info(Color::Green)
        .debug(Color::Cyan)
        .trace(Color::Blue);

    // Get instant
    let started = Instant::now();

    // Build the logger
    fern::Dispatch::new()
        .format(move |out, message, record| {
            let elapsed = started.elapsed();
            let secs = elapsed.as_secs();

            out.finish(format_args!(
                "{}[{: >2}m {: >2}.{:03}s] {}: {}",
                format_args!(
                    "\x1B[{}m",
                    colors_line.get_color(&record.level()).to_fg_str()
                ),
                secs / 60,
                secs % 60,
                elapsed.subsec_millis(),
                match record.level() {
                    Level::Info => "INFO",
                    Level::Warn => "WARNING",
                    Level::Error => "ERROR",
                    Level::Debug => "VERBOSE",
                    Level::Trace => "DEBUG",
                },
                format!("{}", message).red()
            ))
        })
        .level(level)
        .chain(std::io::stdout())
        .apply()
        .unwrap()
}

/// Fail gracefully
/// Program will exit with status code 1
fn fail(message: &str) {
    error!("{}", message);
    std::process::exit(1);
}

/// Command-line entrypoint
fn main() {
    let args: Command = Command::parse();

    logger(match args.verbosity.as_str() {
        "silent" => LevelFilter::Off,
        "errors" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "verbose" => LevelFilter::Debug,
        "debug" => LevelFilter::Trace,
        _ => unreachable!(),
    });

    let input = Path::new(&args.input);

    if !input.exists() {
        fail("Input file not found");
    } else if !args.recursive && !input.is_file() {
        fail("Input is not a file - if you want to check a folder, use the '-r' / '--recursive' option");
    } else if args.recursive && !input.is_dir() {
        fail("Input is not a directory but '-r' / '--recursive' option was supplied");
    }

    match check_broken_links(
        input,
        args.recursive,
        args.ignore_header_links,
        args.no_error,
        &mut HashMap::new(),
    ) {
        Ok(0) => info!("OK."),
        Ok(errors @ _) => {
            let message = format!(
                "Found {} broken or invalid link{}!",
                errors,
                if errors > 1 { "s" } else { "" }
            );

            if args.no_error {
                warn!("{}", message);
            } else {
                fail(&message);
            }
        }
        Err(err) => fail(&err),
    }
}
