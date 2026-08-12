use std::io::{Write, stdin, stdout};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::process::exit;
use std::thread;

use argh::FromArgs;
use serde::Deserialize;
use torrent_utils::{
    CreateOptions, FileTree, ProgressReporter, Torrent, VerificationOptions, VerificationReport,
    WalkMode, human_size,
};

mod cli_output;

use crate::cli_output::{CliProgress, blueprintln, errprint, errprintln, greenprintln};

const DEF_PIECE_SIZE: u8 = 24; // 1 << 24 = 16777216 bytes = 16 MiB

const NAME_VERSION: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("TORRENTUTILSR_VERSION"));

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

fn wait_for_enter(wait: bool) {
    if wait {
        print!("Press Enter to exit...");
        let _ = stdout().flush();
        let _ = stdin().read_line(&mut String::new());
    }
}

fn confirm_overwrite(torrent_path: &str) -> bool {
    print!("Torrent file '{torrent_path}' already exists. Overwrite? [y/N]: ");
    let _ = stdout().flush();

    let mut answer = String::new();
    if stdin().read_line(&mut answer).is_err() {
        return false;
    }

    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn print_file_tree(torrent: &Torrent) {
    match torrent.get_info() {
        Some(info) => {
            if let Some(name) = &info.name {
                println!("{name}");
            }
            if let Some(files) = &info.files {
                print!("{}", FileTree::build(files));
            } else if let Some(length) = info.length {
                println!("  [Single file, {} ({})]", length, human_size(length));
            } else {
                println!("  [No files information available]");
            }
        }
        None => println!("[No torrent info available]"),
    }
}

fn print_verification_report(report: &VerificationReport) {
    let failed_piece_count = report.failed_pieces.len();
    let failed_file_count = report.failed_files.len();

    println!("Verification Result:");
    println!(
        "Pieces: {:8} total = {:8} passed + {failed_piece_count:8} failed",
        report.total_pieces,
        report.passed_pieces()
    );
    println!(
        "Files:  {:8} total = {:8} passed + {failed_file_count:8} failed",
        report.total_files,
        report.passed_files()
    );

    if report.is_ok() {
        println!("All files are OK.");
    } else {
        println!("\nSome files failed verification:");
        for file in &report.failed_files {
            let known_issue = if file.missing_or_size_mismatch {
                " [missing or size mismatch]"
            } else {
                ""
            };
            println!(
                "- {} ({} [{}]){}",
                file.path,
                file.length,
                human_size(file.length),
                known_issue
            );
        }
    }
}

fn main() {
    let args: Args = argh::from_env();

    if args.version {
        println!("{NAME_VERSION}");
        return;
    }

    let mut config: Config = std::fs::read_to_string(&args.config)
        .map_err(|_| ())
        .and_then(|content| {
            toml::from_str::<Config>(&content)
                .map_err(|_| ())
                .inspect(|_| {
                    if !args.quiet {
                        greenprintln!("I:", " Config loaded.");
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

    match args.input.len() {
        1 => {
            let input = &args.input[0];
            if input.ends_with(".torrent") {
                // show info
                if !args.quiet {
                    greenprintln!("I:", " Info mode.");
                    blueprintln!("Torrent:", " {input}");
                }
                match Torrent::read_torrent(input.clone()) {
                    Ok(torrent) => {
                        if args.print_tree {
                            print_file_tree(&torrent);
                        } else {
                            println!("{torrent}");
                        }
                    }
                    Err(e) => {
                        errprintln!("Error reading torrent file:", " {e}");
                        wait_for_enter(config.wait_exit);
                        exit(1);
                    }
                }
            } else {
                // create mode
                if !args.quiet {
                    greenprintln!("I:", " Create mode.");
                }
                config.piece_size = args.piece_size.unwrap_or(config.piece_size);

                let tr_config = CreateOptions {
                    piece_length: 1usize
                        << match config.piece_size {
                            14..=27 => config.piece_size,
                            _ => {
                                errprintln!("Error:", " Piece size must be between 14 and 27.");
                                wait_for_enter(config.wait_exit);
                                exit(1);
                            }
                        },
                    private: args.private || config.private,
                    n_jobs: config.n_jobs,
                    walk_mode: match args.walk_mode.unwrap_or(config.walk_mode) {
                        0 => WalkMode::Default,
                        1 => WalkMode::Alphabetical,
                        2 => WalkMode::BreadthFirstAlphabetical,
                        3 => WalkMode::BreadthFirstLevel,
                        4 => WalkMode::FileSize,
                        _ => {
                            errprintln!("Error:", " Invalid walk mode.");
                            wait_for_enter(config.wait_exit);
                            exit(1);
                        }
                    },
                    source: args.source.or(config.source).filter(|s| !s.is_empty()),
                };

                config.tracker_list = if !args.announce.is_empty() {
                    if args.announce.iter().any(|s| s.is_empty()) {
                        Vec::new()
                    } else {
                        args.announce.clone()
                    }
                } else {
                    config.tracker_list
                };

                let torrent_path = match args.output {
                    Some(ref path) => {
                        if path.ends_with(".torrent") {
                            let path_obj = Path::new(path);
                            if path_obj.is_absolute() || path.contains(MAIN_SEPARATOR) {
                                path.clone()
                            } else {
                                let target_path = Path::new(input);
                                let parent_path =
                                    target_path.parent().unwrap_or_else(|| Path::new("."));
                                parent_path.join(path).to_string_lossy().to_string()
                            }
                        } else {
                            errprint!("Error:", " Output path must end with .torrent");
                            wait_for_enter(config.wait_exit);
                            exit(1);
                        }
                    }
                    None => format!("{input}.torrent"),
                };

                let mut force_overwrite = args.force;
                if !force_overwrite && Path::new(&torrent_path).exists() {
                    if config.confirm_overwrite && confirm_overwrite(&torrent_path) {
                        force_overwrite = true;
                    } else if config.confirm_overwrite {
                        eprintln!("Creation cancelled; existing torrent file was not changed.");
                        wait_for_enter(config.wait_exit);
                        exit(1);
                    } else {
                        errprintln!(
                            "Error writing torrent file:",
                            " File already exists, use -f to overwrite"
                        );
                        wait_for_enter(config.wait_exit);
                        exit(1);
                    }
                }

                if !args.quiet {
                    blueprintln!("Target:", "  {input}");
                    blueprintln!("Torrent:", " {torrent_path}");
                    blueprintln!(
                        "Piece Length:",
                        " {} bytes [{}]",
                        tr_config.piece_length,
                        human_size(tr_config.piece_length)
                    );
                    if tr_config.private {
                        println!("Private Torrent");
                    }
                }

                let announce_list: Vec<Vec<String>> = config
                    .tracker_list
                    .iter()
                    .map(|url| vec![url.clone()])
                    .collect();

                let mut torrent = Torrent::new(
                    if announce_list.is_empty() {
                        None
                    } else {
                        Some(announce_list[0][0].clone())
                    },
                    if announce_list.is_empty() {
                        None
                    } else {
                        Some(announce_list)
                    },
                    args.comment,
                    Some(NAME_VERSION.to_string()),
                    if args.no_date {
                        None
                    } else {
                        Some(chrono::Local::now().timestamp())
                    },
                    Some(String::from("UTF-8")),
                );

                let progress = (!args.quiet).then(CliProgress::new);
                let progress = progress
                    .as_ref()
                    .map(|progress| progress as &dyn ProgressReporter);
                if let Err(e) = torrent.create_torrent(input, &tr_config, progress) {
                    errprintln!("Error creating torrent:", " {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }

                if let Err(e) = torrent.write_to_file(torrent_path, force_overwrite) {
                    errprintln!("Error writing torrent file:", " {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            }
        }
        2 => {
            let inputs = &args.input;
            let (torrent_path, target_path) = if inputs[0].ends_with(".torrent") {
                (inputs[0].clone(), inputs[1].clone())
            } else if inputs[1].ends_with(".torrent") {
                (inputs[1].clone(), inputs[0].clone())
            } else {
                errprintln!(
                    "Error:",
                    " Please provide a .torrent file as one of the arguments."
                );
                wait_for_enter(config.wait_exit);
                exit(1);
            };
            if !args.quiet {
                greenprintln!("I:", " Verify mode.");
                blueprintln!("Target:", "  {target_path}");
                blueprintln!("Torrent:", " {torrent_path}");
            }

            let torrent = match Torrent::read_torrent(torrent_path) {
                Ok(t) => t,
                Err(e) => {
                    errprintln!("Error reading torrent file:", " {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            };
            let tr_info = match torrent.get_info() {
                Some(info) => info,
                None => {
                    errprintln!(
                        "Error:",
                        " Torrent file does not contain valid info section"
                    );
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            };
            let base_path = Path::new(&target_path);
            let name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let tr_name = tr_info.get_name().unwrap_or(String::from("<unknown>"));
            if name != tr_name {
                errprintln!(
                    "Error:",
                    " Target name '{name}' does not match torrent name '{tr_name}'"
                );
                wait_for_enter(config.wait_exit);
                exit(1);
            } else {
                let full_path = base_path.parent().unwrap_or_else(|| Path::new(""));
                if !full_path.join(&tr_name).exists() {
                    errprintln!(
                        "Error:",
                        " Target path '{}' does not exist",
                        full_path.join(&tr_name).display()
                    );
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            }

            let progress = (!args.quiet).then(CliProgress::new);
            let progress = progress
                .as_ref()
                .map(|progress| progress as &dyn ProgressReporter);
            let verify_options = VerificationOptions {
                n_jobs: config.n_jobs,
            };
            match tr_info.verify(&target_path, &verify_options, progress) {
                Ok(report) => print_verification_report(&report),
                Err(e) => {
                    errprintln!("Error during verification:", " {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            }
        }
        _ => {
            errprintln!(
                "Error:",
                " Please provide one target (create), one .torrent (info), or a .torrent plus target (verify)."
            );
            wait_for_enter(config.wait_exit);
            exit(1);
        }
    }

    wait_for_enter(config.wait_exit);
}
