use argh::FromArgs;
use serde::Deserialize;
use std::io::{Write, stdin, stdout};
use std::path::{MAIN_SEPARATOR, Path};
use std::process::exit;

mod torrent;
mod utils;

use torrent::{Torrent, WalkMode};

const DEF_PIECE_SIZE: u8 = 20; // 1 << 16 = 65536 bytes = 64 KiB

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default)]
    wait_exit: bool,

    #[serde(default)]
    walk_mode: u8,

    #[serde(default)]
    private: bool,

    #[serde(default = "def_piece_size")]
    piece_size: u8,

    #[serde(default)]
    tracker_list: Vec<String>,
}

fn def_piece_size() -> u8 {
    DEF_PIECE_SIZE
}

/// A utility for working with torrent files.
#[derive(FromArgs)]
#[argh(help_triggers("-h", "--help"))]
struct Args {
    /// torrent/target path or both
    #[argh(positional)]
    input: Vec<String>,

    /// config file
    #[argh(option, short = 'g', default = "String::from(\"config.toml\")")]
    config: String,

    /// output path or torrent name (only for create mode)
    #[argh(option, short = 'o')]
    output: Option<String>,

    /// piece size (1 << n, 14..=27), overrides config [default: 20]
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

    /// walk mode [default: 0]
    #[argh(option, short = 'w')]
    walk_mode: Option<u8>,

    /// force overwrite
    #[argh(switch, short = 'f')]
    force: bool,

    /// hide progress bar and other non-error output
    #[argh(switch, short = 'q')]
    quiet: bool,

    /// wait for Enter key before exiting
    #[argh(switch, short = 'e')]
    wait_exit: bool,
}

fn wait_for_enter(wait: bool) {
    if wait {
        print!("Press Enter to exit...");
        let _ = stdout().flush();
        let _ = stdin().read_line(&mut String::new());
    }
}

fn main() {
    let args: Args = argh::from_env();

    let mut config: Config = std::fs::read_to_string(&args.config)
        .ok()
        .and_then(|content| {
            toml::from_str::<Config>(&content).ok().inspect(|_| {
                println!("Config loaded.");
            })
        })
        .unwrap_or_default();

    config.wait_exit = args.wait_exit || config.wait_exit;

    match args.input.len() {
        1 => {
            let input = &args.input[0];
            if input.ends_with(".torrent") {
                // show info
                match Torrent::read_torrent(input.clone()) {
                    Ok(torrent) => println!("{torrent}"),
                    Err(e) => {
                        eprintln!("Error reading torrent file: {e}");
                        wait_for_enter(config.wait_exit);
                        exit(1);
                    }
                }
            } else {
                // create mode
                config.piece_size = args.piece_size.unwrap_or(config.piece_size);
                let piece_length = 1usize
                    << match config.piece_size {
                        14..=27 => config.piece_size,
                        _ => {
                            eprintln!("Error: Piece size must be between 14 and 27.");
                            wait_for_enter(config.wait_exit);
                            exit(1);
                        }
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
                config.walk_mode = args.walk_mode.unwrap_or(config.walk_mode);
                config.private = args.private || config.private;

                let walk_mode = match config.walk_mode {
                    0 => WalkMode::Default,
                    1 => WalkMode::Alphabetical,
                    2 => WalkMode::BreadthFirstAlphabetical,
                    3 => WalkMode::BreadthFirstLevel,
                    4 => WalkMode::FileSize,
                    _ => {
                        eprintln!("Error: Invalid walk mode.");
                        wait_for_enter(config.wait_exit);
                        exit(1);
                    }
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
                            eprint!("Error: Output path must end with .torrent");
                            wait_for_enter(config.wait_exit);
                            exit(1);
                        }
                    }
                    None => format!("{input}.torrent"),
                };

                if !args.quiet {
                    println!("Target:  {input}");
                    println!("Torrent: {torrent_path}");
                    println!(
                        "Piece Length: {} bytes [{}]",
                        piece_length,
                        utils::human_size(piece_length)
                    );
                    if config.private {
                        println!("Private Torrent");
                    }
                    println!();
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
                    Some(format!(
                        "{} {}",
                        env!("CARGO_PKG_NAME"),
                        env!("CARGO_PKG_VERSION")
                    )),
                    if args.no_date {
                        None
                    } else {
                        Some(chrono::Local::now().timestamp())
                    },
                    Some(String::from("UTF-8")),
                );

                if let Err(e) = torrent.create_torrent(
                    input.clone(),
                    piece_length,
                    config.private,
                    args.quiet,
                    walk_mode,
                ) {
                    eprintln!("Error creating torrent: {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }

                if let Err(e) = torrent.write_to_file(torrent_path, args.force) {
                    eprintln!("Error writing torrent file: {e}");
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
                eprintln!("Error: Please provide a .torrent file as one of the arguments.");
                wait_for_enter(config.wait_exit);
                exit(1);
            };

            println!("Target:  {target_path}");
            println!("Torrent: {torrent_path}");

            let torrent = match Torrent::read_torrent(torrent_path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error reading torrent file: {e}");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            };
            let tr_info = match torrent.get_info() {
                Some(info) => info,
                None => {
                    eprintln!("Error: Torrent file does not contain valid info section");
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            };
            let base_path = Path::new(&target_path);
            let name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let tr_name = tr_info.get_name().unwrap_or("<unknown>".to_string());
            if name != tr_name {
                eprintln!("Error: Target name '{name}' does not match torrent name '{tr_name}'");
                wait_for_enter(config.wait_exit);
                exit(1);
            } else {
                let full_path = base_path.parent().unwrap_or_else(|| Path::new(""));
                if !full_path.join(&tr_name).exists() {
                    eprintln!(
                        "Error: Target path '{}' does not exist",
                        full_path.join(&tr_name).display()
                    );
                    wait_for_enter(config.wait_exit);
                    exit(1);
                }
            }

            if let Err(e) = tr_info.verify(target_path) {
                eprintln!("Error during verification: {e}");
                wait_for_enter(config.wait_exit);
                exit(1);
            }
        }
        _ => {
            eprintln!(
                "Error: Please provide one target (create), one .torrent (info), or a .torrent plus target (verify)."
            );
            wait_for_enter(config.wait_exit);
            exit(1);
        }
    }

    wait_for_enter(config.wait_exit);
}
