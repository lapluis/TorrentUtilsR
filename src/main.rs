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

enum ProcessMode {
    Create,
    Read,
}

fn main() {
    let args = Args::parse();

    #[cfg(debug_assertions)]
    {
        println!("{args:?}");
    }

    let (torrent_path, target_path, process_mode) = match args.input {
        Some(ref inputs) if inputs.len() == 1 => {
            let input = &inputs[0];
            if input.ends_with(".torrent") {
                (input.clone(), String::from("None"), ProcessMode::Read)
            } else {
                (
                    format!("{}.torrent", input.clone()),
                    input.clone(),
                    ProcessMode::Create,
                )
            }
        }
        Some(ref _inputs) => (
            String::from("None"),
            String::from("None"),
            ProcessMode::Create,
        ),
        None => (
            String::from("None"),
            String::from("None"),
            ProcessMode::Create,
        ),
    };

    match process_mode {
        ProcessMode::Create => {
            let announce_list = match args.announce {
                Some(ref urls) if !urls.is_empty() => vec![urls.clone()],
                _ => Vec::new(),
            };

            let torrent = torrent::Torrent::create_torrent(torrent::TorrentInfo {
                target_path,
                piece_size: args.piece_size as u64,
                private: args.private,
                encoding: String::from("UTF-8"),
                announce: if announce_list.is_empty() {
                    None
                } else {
                    Some(announce_list[0][0].clone())
                },
                announce_list: if announce_list.is_empty() {
                    None
                } else {
                    Some(announce_list)
                },
                created_by: format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
                creation_date: chrono::offset::Utc::now().timestamp() as u64,
                comment: args.comment,
            });

            torrent
                .write_to_file(torrent_path)
                .expect("Failed to write torrent file");
        }
        ProcessMode::Read => {
            let _torrent = torrent::Torrent::read_torrent(torrent_path.clone()).unwrap();
            // TODO
        }
    };
}
