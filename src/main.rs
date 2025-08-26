use clap::Parser;
use std::process;

mod torrent;

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A utility for working with torrent files.", long_about = None)]
struct Args {
    /// Torrent/Target Path or Both
    input: Option<Vec<String>>,

    // Piece Size
    #[arg(short = 'l', long = "piece-size", default_value_t = 16)]
    piece_size: u32,

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
                    torrent.create_torrent(input.clone(), args.piece_size as u64, args.private)
                {
                    eprintln!("Error creating torrent: {e}");
                    process::exit(1);
                }

                if let Err(e) = torrent.write_to_file(format!("{input}.torrent"), args.force) {
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
