use clap::Parser;

mod torrent;

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A utility for working with torrent files", long_about = None)]
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

    /// Force overwrite
    #[arg(short = 'f', long)]
    force: bool,
}

fn main() {
    let args = Args::parse();

    // print args for debugging
    println!("{args:?}");

    // if only give on input, treat it according to its extension
    let (torrent_path, target_path, process_mode) = match args.input {
        Some(ref inputs) if inputs.len() == 1 => {
            let input = &inputs[0];
            if input.ends_with(".torrent") {
                (
                    input.clone(),
                    String::from("None"),
                    torrent::ProcessMode::Verify,
                )
            } else {
                (
                    format!("{}.torrent", input.clone()),
                    input.clone(),
                    torrent::ProcessMode::Create,
                )
            }
        }
        Some(ref _inputs) => (
            String::from("None"),
            String::from("None"),
            torrent::ProcessMode::Create,
        ),
        None => (
            String::from("None"),
            String::from("None"),
            torrent::ProcessMode::Create,
        ),
    };

    let piece_size = args.piece_size;
    let announce_list = args.announce.clone().unwrap_or_default();

    match process_mode {
        torrent::ProcessMode::Create => {
            // pass
            let torrent = torrent::Torrent::new(
                torrent_path,
                Some(target_path),
                Some(piece_size as u64),
                process_mode,
                Some(announce_list),
                None,
                Some(chrono::Local::now().timestamp() as u64),
                Some(format!(
                    "{} {}",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_VERSION")
                )),
                args.private,
                String::from("UTF-8"),
            );

            torrent
                .write_to_file()
                .expect("Failed to write torrent file");
        }
        torrent::ProcessMode::Verify => {
            // pass
        }
    };
}
