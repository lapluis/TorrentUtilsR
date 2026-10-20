use std::io::{Write, stdin, stdout};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;

use argh::FromArgs;
use serde::Deserialize;

mod cli_output;
mod commands;
mod render;

use crate::cli_output::outputln;

const DEF_PIECE_SIZE: u8 = 24; // 1 << 24 = 16777216 bytes = 16 MiB

pub(crate) const NAME_VERSION: &str =
    concat!(env!("CARGO_PKG_NAME"), " ", env!("TORRENTUTILSR_VERSION"));

#[derive(Deserialize)]
struct Config {
    #[serde(default)]
    wait_exit: bool,

    #[serde(default)]
    confirm_overwrite: bool,

    #[serde(default = "default_n_jobs")]
    n_jobs: usize,

    #[serde(default)]
    walk_mode: u8,

    #[serde(default)]
    private: bool,

    #[serde(default = "def_piece_size")]
    piece_size: u8,

    #[serde(default)]
    source: Option<String>,

    #[serde(default)]
    tracker_list: Vec<String>,
}

const fn def_piece_size() -> u8 {
    DEF_PIECE_SIZE
}

const fn default_n_jobs() -> usize {
    1
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wait_exit: false,
            confirm_overwrite: false,
            n_jobs: 1,
            walk_mode: 0,
            private: false,
            piece_size: DEF_PIECE_SIZE,
            source: None,
            tracker_list: Vec::new(),
        }
    }
}

/// A utility for working with torrent files.
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help"))]
struct Args {
    /// torrent/target path or both
    #[argh(positional)]
    input: Vec<String>,

    /// config file
    #[argh(option, short = 'g', default = "get_config_path()")]
    config: String,

    /// output path or torrent name (only for create mode)
    #[argh(option, short = 'o')]
    output: Option<String>,

    /// piece size (1 << n, 14..=27), overrides config [default: 24]
    #[argh(option, short = 'l')]
    piece_size: Option<u8>,

    /// announce URLs, multiple allowed, overrides config (\"\" to clear)
    #[argh(option, short = 'a')]
    announce: Vec<String>,

    /// private torrent, overrides config
    #[argh(switch, short = 'p')]
    private: bool,

    /// comment
    #[argh(option, short = 'c')]
    comment: Option<String>,

    /// no creation date
    #[argh(switch, short = 'd')]
    no_date: bool,

    /// torrent source
    #[argh(option, short = 's')]
    source: Option<String>,

    /// walk mode [default: 0]
    #[argh(option, short = 'w')]
    walk_mode: Option<u8>,

    /// force overwrite
    #[argh(switch, short = 'f')]
    force: bool,

    /// number of threads to use (only for verify mode) [default: 1]
    #[argh(option, short = 'j')]
    n_jobs: Option<usize>,

    /// hide progress bar and other non-error output
    #[argh(switch, short = 'q')]
    quiet: bool,

    /// print torrent file tree, only for info mode
    #[argh(switch, short = 't')]
    print_tree: bool,

    /// wait for Enter key before exiting
    #[argh(switch, short = 'e')]
    wait_exit: bool,

    /// print version info and exit
    #[argh(switch, short = 'v')]
    version: bool,
}

fn get_config_path() -> String {
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    exe_dir.join("config.toml").to_string_lossy().to_string()
}

fn load_config(args: &Args) -> Config {
    let mut config: Config = std::fs::read_to_string(&args.config)
        .map_err(|_| ())
        .and_then(|content| {
            toml::from_str::<Config>(&content)
                .map_err(|_| ())
                .inspect(|_| {
                    if !args.quiet {
                        outputln!(plain, "I:", " Config loaded.");
                    }
                })
        })
        .unwrap_or_default();

    config.wait_exit = args.wait_exit || config.wait_exit;

    config.n_jobs = args.n_jobs.unwrap_or(config.n_jobs).clamp(
        1,
        thread::available_parallelism()
            .map(|p| p.get())
            .unwrap_or(1),
    );

    config
}

fn wait_for_enter(wait: bool) {
    if wait {
        print!("Press Enter to exit...");
        let _ = stdout().flush();
        let _ = stdin().read_line(&mut String::new());
    }
}

fn main() -> ExitCode {
    let args: Args = argh::from_env();

    if args.version {
        println!("{NAME_VERSION}");
        return ExitCode::SUCCESS;
    }

    let config = load_config(&args);
    let result = commands::run(&args, &config);

    if let Err(error) = &result {
        error.report();
    }
    wait_for_enter(config.wait_exit);

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
