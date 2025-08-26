use clap::Parser;
use std::process;

mod torrent;
mod utils;

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A utility for working with torrent files.", long_about = None)]
struct Args {
    /// Torrent/Target Path or Both
    input: Option<Vec<String>>,

    /// Output Path (only for create mode)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Piece Size [11, 24]
    #[arg(short = 'l', long = "piece-size", default_value_t = 16)]
    piece_size: u16,

    /// Announce URLs, multiple allowed
    #[arg(short = 'a', long)]
    announce: Option<Vec<String>>,

    /// Private Torrent
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

    if args.piece_size < 11 || args.piece_size > 24 {
        eprintln!("Error: Piece size must be between 11 and 24 (inclusive).");
        process::exit(1);
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
                let piece_length = 1usize << args.piece_size;
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
                        piece_length,
                        utils::human_size(piece_length)
                    );
                    println!();
                }

                let announce_list = match args.announce {
                    Some(ref urls) if !urls.is_empty() => vec![urls.clone()],
                    _ => Vec::new(),
                };

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

                if let Err(e) =
                    torrent.create_torrent(input.clone(), piece_length, args.private, args.quiet)
                {
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
