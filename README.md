# TorrentUtilsR

A fast and reliable command-line utility for creating, reading, and verifying BitTorrent files, written in Rust.

## Features

- **Create torrents** from files or directories
- **Read torrent files** and display comprehensive information
- **Verify torrents** against existing files with detailed reporting
- **Configurable** with TOML configuration file support

## Installation

### From Source

```bash
git clone https://github.com/lapluis/TorrentUtilsR.git
cd TorrentUtilsR
cargo build --release
```

## Usage

### Creating Torrents

Create a torrent from a file or directory:

```bash
# Create torrent from a file
TorrentUtilsR path/to/file.txt

# Create torrent from a directory
TorrentUtilsR path/to/directory

# Specify output location
TorrentUtilsR path/to/data -o my-torrent.torrent

# Create private torrent with custom piece size
TorrentUtilsR path/to/data -p -l 18
```

### Reading Torrent Information

Display detailed information about a torrent file:

```bash
TorrentUtilsR example.torrent

# Print torrent information with file tree structure
TorrentUtilsR example.torrent --print-tree
```

### Verifying Torrents

Verify that files match their torrent:

```bash
# Verify torrent against files (order doesn't matter)
TorrentUtilsR example.torrent path/to/data
TorrentUtilsR path/to/data example.torrent
```

### Command Line Options

```
Usage: TorrentUtilsR [<input...>] [-g <config>] [-o <output>] [-l <piece-size>] [-a <announce...>] [-p] [-c <comment>] [-d] [-s <source>] [-w <walk-mode>] [-f] [-j <n-jobs>] [-q] [-t] [-e]

A utility for working with torrent files.

Positional Arguments:
  input             torrent/target path or both

Options:
  -g, --config      config file
  -o, --output      output path or torrent name (only for create mode)
  -l, --piece-size  piece size (1 << n, 14..=27), overrides config [default: 24]
  -a, --announce    announce URLs, multiple allowed, overrides config ("" to
                    clear)
  -p, --private     private torrent, overrides config
  -c, --comment     comment
  -d, --no-date     no creation date
  -s, --source      torrent source
  -w, --walk-mode   walk mode [default: 0]
  -f, --force       force overwrite
  -j, --n-jobs      number of threads to use (only for verify mode) [default: 1]
  -q, --quiet       hide progress bar and other non-error output
  -t, --print-tree  print torrent file tree, only for info mode
  -e, --wait-exit   wait for Enter key before exiting
  -h, --help        display usage information
  -v, --version     print version info and exit
```

#### Walk Modes

The `-w, --walk-mode` option controls how files are ordered when creating torrents from directories:

- **0 (Default)**: Standard directory traversal order
- **1 (Alphabetical)**: Sort files alphabetically
- **2 (Breadth-First Alphabetical)**: Breadth-first traversal with alphabetical sorting (TorrentUtils compatible)
- **3 (Breadth-First Level)**: Breadth-first traversal by directory level (qBittorrent compatible)
- **4 (File Size)**: Sort files by size

## Configuration

TorrentUtilsR supports configuration via a TOML file. By default, it looks for `config.toml` in the current directory.

### Example Configuration

```toml
# config.toml
wait_exit = true
confirm_overwrite = false
n_jobs = 4
walk_mode = 0
private = false
piece_size = 22 # 22 -> 4 MiB pieces

source = "ExampleSource"

tracker_list = [
    "http://nyaa.tracker.wf:7777/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.opentrackr.org:1337/announce",
]
```

### Configuration Options

- **`wait_exit`**: Boolean, wait for Enter key before exiting
- **`confirm_overwrite`**: Boolean, ask before overwriting an existing torrent file (default: false). Enter `y` or `yes` to overwrite; any other response cancels creation. When disabled, an existing output file causes an error before torrent creation starts. `--force` bypasses the check
- **`n_jobs`**: Integer, number of threads to use for verify mode (default: 1)
- **`walk_mode`**: Integer (0-4), default file walking mode for directories
- **`private`**: Boolean, creates private torrents by default
- **`piece_size`**: Integer, piece size exponent (14-27), piece length will be 2^piece_size bytes
- **`tracker_list`**: Array of tracker URLs to include in created torrents
- **`source`**: Optional string, torrent source written into the torrent info metadata

## Examples

### Basic Torrent Creation

```bash
# Create a torrent for a movie file
TorrentUtilsR "My Movie.mkv"

# This creates "My Movie.mkv.torrent" with default settings
```

### Advanced Torrent Creation

```bash
# Create private torrent with custom settings
TorrentUtilsR "My Series/" \
  --output "My-Series-Complete.torrent" \
  --private \
  --piece-size 22 \
  --comment "Complete series collection" \
  --announce "http://private-tracker.example.com/announce"

# Create torrent and set torrent source metadata
TorrentUtilsR "My Series/" \
  --output "My-Series-Complete-with-source.torrent" \
  --private \
  --piece-size 22 \
  --comment "Complete series collection" \
  --source "ExampleSource" \
  --announce "http://private-tracker.example.com/announce"

# Create torrent with alphabetical file ordering
TorrentUtilsR "My Directory/" \
  --walk-mode 1 \
  --output "sorted-torrent.torrent"

# Create torrent with qBittorrent-compatible file ordering
TorrentUtilsR "My Directory/" \
  --walk-mode 3 \
  --output "qbittorrent-compatible.torrent"
```

### Verification Example

```bash
# Verify downloaded files against torrent
TorrentUtilsR ubuntu-22.04.torrent ~/Downloads/ubuntu-22.04/

# Output shows verification results:
# Verification Result:
# Pieces:     1234 total =     1234 passed +        0 failed
# Files:        15 total =       15 passed +        0 failed
# All files are OK.
```

## Testing

The test suite contains two complementary layers:

- `tests/library_api.rs` tests the reusable library API, including creation, serialization,
  parsing, progress reporting, single-file and multi-file verification, malformed input, empty
  files, overwrite behavior, and deterministic formatting.
- `tests/cli_compat.rs` is a black-box compatibility suite covering CLI help and version output,
  configuration, metadata, file trees, create/info/verify workflows, overwrite handling, invalid
  arguments, empty files, and malformed piece data.

Run the same checks used by CI with:

```bash
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo test --locked --no-default-features --lib --test library_api
cargo clippy --locked --all-targets --all-features -- -D warnings
```

The CLI compatibility suite normally uses the binary built by Cargo. To test another build or
branch, set `TORRENTUTILSR_TEST_BIN` to that executable before running
`cargo test --test cli_compat`.

## Library API

The project also exposes a `torrent_utils` library target. The library handles torrent creation,
parsing, serialization, and verification, while terminal interaction remains in the CLI.

```rust
use torrent_utils::{CreateOptions, Torrent, WalkMode};

let options = CreateOptions {
    piece_length: 1 << 20,
    private: false,
    n_jobs: 4,
    walk_mode: WalkMode::Alphabetical,
    source: None,
};

let mut torrent = Torrent::new(None, None, None, None, None, Some("UTF-8".into()));
torrent.create_torrent("path/to/data", &options, None)?;
torrent.write_to_file("data.torrent", false)?;
# Ok::<(), torrent_utils::TrError>(())
```

Pass an implementation of `ProgressReporter` instead of `None` when an application wants progress
updates. Verification returns a `VerificationReport`; rendering that report is the caller's
responsibility. Library-only consumers can disable CLI dependencies with
`default-features = false`.

## Thanks to

[airium/TorrentUtils](https://github.com/airium/TorrentUtils)
