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
Usage: TorrentUtilsR [OPTIONS] [INPUT]...

Arguments:
  [INPUT]...  Torrent/Target Path or Both

Options:
  -g, --config <CONFIG>        Config file [default: config.toml]
  -o, --output <OUTPUT>        Output Path (only for create mode)
  -l, --piece-size <PIECE_SIZE> Piece Size (1 << n, [11, 24]), overrides config [default: 16]
  -a, --announce <ANNOUNCE>    Announce URLs, multiple allowed, overrides config ("" to clear)
  -p, --private               Private Torrent, overrides config
  -c, --comment <COMMENT>     Comment
  -d, --no-date               No creation date
  -f, --force                 Force overwrite
  -q, --quiet                 Hide progress bar and other non-error output
  -h, --help                  Print help
  -V, --version               Print version
```

## Configuration

TorrentUtilsR supports configuration via a TOML file. By default, it looks for `config.toml` in the current directory.

### Example Configuration

```toml
# config.toml
private = false
piece_length = 131072  # 128 KiB

tracker_list = [
    "http://tracker1.example.com:8080/announce",
    "http://tracker2.example.com:8080/announce",
    "udp://tracker3.example.com:1337/announce"
]
```

### Configuration Options

- **`private`**: Boolean, creates private torrents by default
- **`piece_length`**: Integer, default piece size in bytes (must be power of 2)
- **`tracker_list`**: Array of tracker URLs to include in created torrents

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
  --piece-size 20 \
  --comment "Complete series collection" \
  --announce "http://private-tracker.example.com/announce"
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

## Thanks to

https://github.com/airium/TorrentUtils
