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
Usage: TorrentUtilsR.exe [<input...>] [-g <config>] [-o <output>] [-l <piece-size>] [-a <announce...>] [-p] [-c <comment>] [-d] [-w <walk-mode>] [-f] [-q] [-t] [-e]

A utility for working with torrent files.

Positional Arguments:
  input                       torrent/target path or both

Options:
  -g, --config <config>       config file
  -o, --output <output>       output path or torrent name (only for create mode)
  -l, --piece-size <piece-size> piece size (1 << n, 14..=27), overrides config [default: 20]
  -a, --announce <announce...> announce URLs, multiple allowed, overrides config ("" to clear)
  -p, --private               private torrent, overrides config
  -c, --comment <comment>     comment
  -d, --no-date               no creation date
  -w, --walk-mode <walk-mode> walk mode [default: 0]
  -f, --force                 force overwrite
  -q, --quiet                 hide progress bar and other non-error output
  -t, --print-tree            print torrent file tree, only for info mode
  -e, --wait-exit             wait for Enter key before exiting
  -h, --help                  display usage information
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
walk_mode = 0
private = false
piece_size = 22

tracker_list = [
    "http://nyaa.tracker.wf:7777/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.opentrackr.org:1337/announce",
]
```

### Configuration Options

- **`wait_exit`**: Boolean, wait for Enter key before exiting
- **`walk_mode`**: Integer (0-4), default file walking mode for directories
- **`private`**: Boolean, creates private torrents by default
- **`piece_size`**: Integer, piece size exponent (14-27), piece length will be 2^piece_size bytes
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
  --piece-size 22 \
  --comment "Complete series collection" \
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

## Thanks to

https://github.com/airium/TorrentUtils
