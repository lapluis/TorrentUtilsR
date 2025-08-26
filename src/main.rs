use clap::Parser;
use serde::Deserialize;
use std::process;

mod torrent;
mod utils;

const DEF_PIECE_SIZE: u8 = 16; // 1 << 16 = 65536 bytes, 512 KiB

#[derive(Debug, Deserialize)]
struct Config {
    private: bool,
    piece_length: usize, // in bytes
    tracker_list: Vec<String>,
}

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A utility for working with torrent files.", long_about = None)]
struct Args {
    /// Torrent/Target Path or Both
    input: Option<Vec<String>>,

    /// Config file
    #[arg(short = 'g', long, default_value = "config.toml")]
    config: String,

    /// Output Path (only for create mode)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Piece Size (1 << n, [11, 24]), overrides config [default: 16]
    #[arg(short = 'l', long = "piece-size")]
    piece_size: Option<u8>,

    /// Announce URLs, multiple allowed, overrides config ("" to clear)
    #[arg(short = 'a', long)]
    announce: Option<Vec<String>>,

    /// Private Torrent, overrides config
    #[arg(short = 'p', long)]
    private: bool,

    /// Comment
    #[arg(short = 'c', long)]
    comment: Option<String>,

    /// No creation date
    #[arg(short = 'd', long)]
    no_date: bool,

    /// Force overwrite
    #[arg(short = 'f', long)]
    force: bool,

    /// Hide progress bar and other non-error output
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

fn main() {
    let args = Args::parse();

    #[cfg(debug_assertions)]
    {
        println!("{args:?}");
    }

    match args.input {
        Some(ref inputs) if inputs.len() == 1 => {
            let input = &inputs[0];
            if input.ends_with(".torrent") {
                // show info
                match torrent::Torrent::read_torrent(input.clone()) {
                    Ok(torrent) => println!("{torrent}"),
                    Err(e) => {
                        eprintln!("Error reading torrent file: {e}");
                        process::exit(1);
                    }
                }
            } else {
                // create mode
                let mut config: Config = std::fs::read_to_string(&args.config)
                    .ok()
                    .and_then(|content| {
                        toml::from_str::<Config>(&content).ok().inspect(|_| {
                            println!("Config loaded successfully");
                        })
                    })
                    .unwrap_or_else(|| Config {
                        private: false,
                        piece_length: 1usize << DEF_PIECE_SIZE,
                        tracker_list: Vec::new(),
                    });

                config.piece_length = match args.piece_size {
                    Some(n) if (11..=24).contains(&n) => 1usize << n,
                    Some(n) => {
                        eprintln!(
                            "Error: Piece size must be between 11 and 24 (inclusive). Got {n}."
                        );
                        process::exit(1);
                    }
                    None => config.piece_length,
                };
                config.private = args.private || config.private;
                config.tracker_list = match args.announce {
                    Some(ref list) if !list.is_empty() && list[0].is_empty() => Vec::new(),
                    Some(ref list) if !list.is_empty() => list.clone(),
                    _ => config.tracker_list,
                };

                let torrent_path = match args.output {
                    Some(ref path) => {
                        if path.ends_with(".torrent") {
                            path.clone()
                        } else {
                            eprint!("Error: Output path must end with .torrent");
                            process::exit(1);
                        }
                    }
                    None => format!("{input}.torrent"),
                };

                if !args.quiet {
                    println!("Target:  {input}");
                    println!("Torrent: {torrent_path}");
                    println!(
                        "Piece Length: {} bytes [{}]",
                        config.piece_length,
                        utils::human_size(config.piece_length)
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

                let mut torrent = torrent::Torrent::new(
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
                    config.piece_length,
                    config.private,
                    args.quiet,
                ) {
                    eprintln!("Error creating torrent: {e}");
                    process::exit(1);
                }

                if let Err(e) = torrent.write_to_file(torrent_path, args.force) {
                    eprintln!("Error writing torrent file: {e}");
                    process::exit(1);
                }
            }
        }
        Some(ref inputs) if inputs.len() == 2 => {
            // two arguments provided, first is .torrent file, second is target path
            // verify mode
            // torrent file and target path may be in any order
            let (torrent_path, target_path) = if inputs[0].ends_with(".torrent") {
                (inputs[0].clone(), inputs[1].clone())
            } else if inputs[1].ends_with(".torrent") {
                (inputs[1].clone(), inputs[0].clone())
            } else {
                eprintln!("Error: Please provide a .torrent file as one of the arguments.");
                process::exit(1);
            };

            println!("Target:  {target_path}");
            println!("Torrent: {torrent_path}");

            let torrent = match torrent::Torrent::read_torrent(torrent_path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error reading torrent file: {e}");
                    process::exit(1);
                }
            };
            let tr_info = match torrent.get_info() {
                Some(info) => info,
                None => {
                    eprintln!("Error: Torrent file does not contain valid info section");
                    process::exit(1);
                }
            };
            let base_path = std::path::Path::new(&target_path);
            let name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let tr_name = tr_info.get_name().unwrap_or("<unknown>".to_string());
            if name != tr_name {
                eprintln!("Error: Target name '{name}' does not match torrent name '{tr_name}'",);
                process::exit(1);
            } else {
                let full_path = base_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""));
                if !full_path.join(&tr_name).exists() {
                    eprintln!(
                        "Error: Target path '{}' does not exist",
                        full_path.join(&tr_name).display()
                    );
                    process::exit(1);
                }
            }

            if let Err(e) = tr_info.verify(target_path) {
                eprintln!("Error during verification: {e}");
                process::exit(1);
            }
        }
        Some(ref _inputs) => {
            eprintln!(
                "Error: Please provide exactly one argument: either a .torrent file to read or a target path to create a torrent."
            );
            process::exit(1);
        }
        None => {
            eprintln!(
                "Error: No input provided. Please provide a .torrent file to read or a target path to create a torrent."
            );
            process::exit(1);
        }
    }
}
