use clap::Parser;

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

    /// Announce URLs
    #[arg(short = 'a', long)]
    announce: Option<Vec<String>>,

    /// Private Torrent
    #[arg(short = 'p', long)]
    private: bool,

    /// Comment
    #[arg(short = 'c', long)]
    comment: Option<String>,

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
                let torrent = torrent::Torrent::read_torrent(input.clone()).unwrap();
                println!("{torrent}");
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
                    Some(chrono::offset::Utc::now().timestamp()),
                    Some(String::from("UTF-8")),
                );

                torrent.create_torrent(input.clone(), args.piece_size as u64, args.private);

                torrent
                    .write_to_file(format!("{input}.torrent"))
                    .expect("Failed to write torrent file");
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
                return;
            };

            let torrent = torrent::Torrent::read_torrent(torrent_path).unwrap();
            let tr_info = torrent.get_info().unwrap();
            tr_info.verify(target_path);
        }
        Some(ref _inputs) => {
            eprintln!(
                "Error: Please provide exactly one argument: either a .torrent file to read or a target path to create a torrent."
            );
        }
        None => {
            eprintln!(
                "Error: No input provided. Please provide a .torrent file to read or a target path to create a torrent."
            );
        }
    }
}
