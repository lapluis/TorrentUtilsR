# TorrentUtils.ps1

`TorrentUtils.ps1` creates, reads, and verifies BitTorrent v1 torrents using only PowerShell 7+
and .NET. It does not require Rust, the compiled executable, or additional modules.

```powershell
# File or directory; default output is <source-path>.torrent beside the source
./TorrentUtils.ps1 -Path 'D:\Downloads\Movie.mkv'
./TorrentUtils.ps1 -Path 'D:\Downloads\Series'

# Private torrent, 4 MiB pieces, and multiple trackers
./TorrentUtils.ps1 -Path 'D:\Downloads\Series' `
    -Output 'D:\Torrents\Series.torrent' `
    -PieceSize 22 -Private -Source 'ExampleSource' `
    -Announce 'https://tracker.example.com/announce', 'udp://tracker.example.com:80/announce'

# Read metadata and list all files, optionally as a tree
./TorrentUtils.ps1 'D:\Torrents\Series.torrent'
./TorrentUtils.ps1 'D:\Torrents\Series.torrent' -PrintTree

# Verify against the actual content directory (or actual file for single-file torrents)
./TorrentUtils.ps1 'D:\Torrents\Series.torrent' 'D:\Downloads\Series'
./TorrentUtils.ps1 'D:\Downloads\Series' 'D:\Torrents\Series.torrent'

# Obtain structured metadata for use in another script
$info = ./TorrentUtils.ps1 'D:\Torrents\Series.torrent' -PassThru
$info.Files | Format-Table Path, Length
```

`-PieceSize` is an exponent from 14 through 27 (default 22, or 4 MiB).
Optional parameters include `-Comment`, `-NoDate`, and `-Force` to replace an
existing output file. Output directories must already exist, and the output must
be outside the source directory. The script returns an object with the output
path, info hash, file count, byte count, piece length, and piece count.

Directory contents include hidden files and are ordered by ordinal relative path.
Pieces continue across file boundaries. Empty files are included; directories
without files are rejected. Symbolic links and reparse points are rejected.
Keep source files unchanged during creation. File data is streamed through one
piece buffer; metadata and the 20-byte hashes for all pieces are held in memory.
The script stages output in a temporary file before renaming it into place.

The operation is selected automatically: one content path creates a torrent, one
`.torrent` path displays information, and two paths verify. Use
`-Mode Create|Info|Verify` to select explicitly, for example to create a torrent
for a file whose name ends in `.torrent`.

Info mode displays the info hash (calculated from the original encoded `info`
bytes), trackers, metadata, sizes, and file paths. `-PrintTree` changes the file
listing to a directory tree. `-PassThru` returns structured metadata instead of
printing it; `Files` contains paths, lengths, and logical offsets, and
`PieceHashes` contains raw SHA-1 bytes. Metadata text must be UTF-8. v2 and hybrid
torrents are rejected. Read/verify accepts piece lengths up to 128 MiB.

Verification reports passed/failed pieces and files. Missing, truncated, and
oversized files fail verification; missing data does not shift subsequent piece
boundaries. Files sharing a failed piece are all reported as affected, since a
piece hash cannot identify which file supplied corrupt bytes. Extra files outside
the torrent file list are ignored. Links/reparse points in target paths and unsafe
paths embedded in torrents are rejected. Verification is sequential and never
modifies content files.

`-PassThru` in Verify mode emits a report with `IsValid`, `FailedPieces` (zero-based
indices), and `FailedFiles` (including reasons). Failed verification throws after
reporting, so `pwsh -File` exits nonzero. When collecting failed reports in a script,
consume pipeline output inside `try`, then handle the terminating error in `catch`.
Success exits zero. Keep content unchanged while verification is running.
