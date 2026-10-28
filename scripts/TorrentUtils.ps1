#requires -Version 7.0
<#
.SYNOPSIS
Creates, reads, and verifies BitTorrent v1 torrents using PowerShell and .NET.
.EXAMPLE
./TorrentUtils.ps1 -Path 'D:\Downloads\Movie.mkv'
.EXAMPLE
./TorrentUtils.ps1 'D:\Downloads\Series.torrent' 'D:\Downloads\Series'
.EXAMPLE
./TorrentUtils.ps1 'D:\Downloads\Series.torrent' -PrintTree
.EXAMPLE
./TorrentUtils.ps1 'D:\Downloads\Series' -Threads 4
.NOTES
PieceSize is an exponent: 22 means 4 MiB. Directory files are ordered by
ordinal relative path. Links and reparse points are rejected. Output must be
outside the source directory. Existing output is preserved unless -Force is set.
Auto mode: content path creates; .torrent path shows info; two paths verify.
TargetPath is the actual content file or directory, not its parent directory.
Info and Verify support -PassThru for structured objects. Verification failure
emits its report before throwing, giving pwsh -File a nonzero exit status.
Only v1 and UTF-8 metadata are supported. Indices in FailedPieces are zero-based.
Create supports -Threads 1..64 (default 1). Parallel hashing uses PowerShell
runspaces with one piece buffer per worker and preserves the single-thread order.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [string] $Path,
    [Parameter(Position = 1)]
    [string] $TargetPath,
    [ValidateSet('Auto', 'Create', 'Info', 'Verify')]
    [string] $Mode = 'Auto',
    [switch] $PrintTree,
    [switch] $PassThru,
    [string] $Output,
    [ValidateRange(14, 27)]
    [int] $PieceSize = 22,
    [string[]] $Announce = @(),
    [switch] $Private,
    [string] $Comment,
    [string] $Source,
    [switch] $NoDate,
    [switch] $Force,
    [ValidateRange(1, 64)]
    [int] $Threads = 1
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Write directly to a stream: hashes are binary, and string lengths are UTF-8 bytes.
function Write-Bencode {
    param([System.IO.Stream] $Stream, [object] $Value)
    if ($Value -is [string] -or $Value -is [byte[]]) {
        [byte[]] $bytes = @()
        if ($Value -is [string]) { $bytes = [System.Text.Encoding]::UTF8.GetBytes($Value) }
        else { $bytes = $Value }
        $prefix = [System.Text.Encoding]::ASCII.GetBytes(
            $bytes.Length.ToString([cultureinfo]::InvariantCulture) + ':')
        $Stream.Write($prefix, 0, $prefix.Length)
        $Stream.Write($bytes, 0, $bytes.Length)
    } elseif ($Value -is [System.Collections.IDictionary]) {
        $Stream.WriteByte(100) # d
        # All metadata keys used here are ASCII; ordinal sorting is byte sorting.
        [string[]] $keys = @($Value.Keys)
        [Array]::Sort($keys, [StringComparer]::Ordinal)
        foreach ($key in $keys) {
            Write-Bencode $Stream $key
            Write-Bencode $Stream $Value[$key]
        }
        $Stream.WriteByte(101) # e
    } elseif ($Value -is [System.Collections.IList]) {
        $Stream.WriteByte(108) # l
        foreach ($item in $Value) { Write-Bencode $Stream $item }
        $Stream.WriteByte(101)
    } elseif ($Value -is [long] -or $Value -is [int]) {
        $bytes = [System.Text.Encoding]::ASCII.GetBytes(
            'i' + $Value.ToString([cultureinfo]::InvariantCulture) + 'e')
        $Stream.Write($bytes, 0, $bytes.Length)
    } else {
        throw "Unsupported Bencode value type: $($Value.GetType().FullName)"
    }
}

function Invoke-ParallelCreationHash {
    param([string[]] $Paths, [long[]] $Lengths, [DateTime[]] $WriteTimes,
        [int] $PieceLength, [long] $TotalBytes, [int] $Threads)
    # Partition the logical byte stream at piece boundaries. Each worker reads a
    # contiguous range, reusing file handles and carrying partial pieces across files.
    $workerCode = {
        param([string[]] $Paths, [long[]] $Lengths, [DateTime[]] $WriteTimes,
            [int] $PieceLength, [long] $StartPiece, [long] $EndPiece,
            [long] $TotalBytes, [int] $WorkerIndex, $ProgressBytes)
        Set-StrictMode -Version Latest
        $ErrorActionPreference = 'Stop'
        $buffer = [byte[]]::new($PieceLength)
        $pieces = [IO.MemoryStream]::new()
        $sha1 = [Security.Cryptography.SHA1]::Create()
        [long] $startByte = $StartPiece * $PieceLength
        [long] $endByte = [Math]::Min($TotalBytes, $EndPiece * $PieceLength)
        [long] $fileOffset = 0
        [long] $processed = 0
        $filled = 0
        try {
            for ($index = 0; $index -lt $Paths.Length; $index++) {
                [long] $fileEnd = $fileOffset + $Lengths[$index]
                if ($fileOffset -ge $endByte) { break }
                if ($fileEnd -gt $startByte -and $Lengths[$index] -gt 0) {
                    [long] $readStart = [Math]::Max($startByte, $fileOffset) - $fileOffset
                    [long] $remaining = [Math]::Min($endByte, $fileEnd) - $fileOffset - $readStart
                    $stream = [IO.File]::Open($Paths[$index], [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
                    try {
                        if ($stream.Length -ne $Lengths[$index]) { throw "Source changed: $($Paths[$index])" }
                        if ($readStart) { [void] $stream.Seek($readStart, [IO.SeekOrigin]::Begin) }
                        while ($remaining -gt 0) {
                            $wanted = [int] [Math]::Min($remaining, $PieceLength - $filled)
                            $count = $stream.Read($buffer, $filled, $wanted)
                            if (!$count) { throw "Source ended before declared length: $($Paths[$index])" }
                            $remaining -= $count
                            $filled += $count
                            $processed += $count
                            if ($filled -eq $PieceLength) {
                                $hash = $sha1.ComputeHash($buffer, 0, $filled)
                                $pieces.Write($hash, 0, $hash.Length)
                                $filled = 0
                                if ($null -ne $ProgressBytes) { $ProgressBytes[$WorkerIndex] = $processed }
                            }
                        }
                        if ([IO.File]::GetLastWriteTimeUtc($Paths[$index]) -ne $WriteTimes[$index]) {
                            throw "Source changed while hashing: $($Paths[$index])"
                        }
                    } finally { $stream.Dispose() }
                }
                $fileOffset = $fileEnd
            }
            if ($processed -ne $endByte - $startByte) { throw 'Incomplete worker byte range.' }
            if ($filled -gt 0) {
                $hash = $sha1.ComputeHash($buffer, 0, $filled)
                $pieces.Write($hash, 0, $hash.Length)
            }
            if ($null -ne $ProgressBytes) { $ProgressBytes[$WorkerIndex] = $processed }
            # One object per worker; binary hashes must not be enumerated into the pipeline.
            [pscustomobject] @{ StartPiece = $StartPiece; Pieces = $pieces.ToArray() }
        } finally {
            $sha1.Dispose()
            $pieces.Dispose()
        }
    }
    [long] $pieceCount = [long] [decimal]::Ceiling([decimal] $TotalBytes / $PieceLength)
    if (!$pieceCount) { return ,([byte[]]::new(0)) }
    $workerCount = [int] [Math]::Min($Threads, $pieceCount)
    [long] $piecesPerWorker = [long] [decimal]::Ceiling([decimal] $pieceCount / $workerCount)
    $progressBytes = $null
    if ($ProgressPreference -ne 'SilentlyContinue' -and $ProgressPreference -ne 'Ignore') {
        $progressBytes = [Collections.Concurrent.ConcurrentDictionary[int, long]]::new()
    }
    $pool = [RunspaceFactory]::CreateRunspacePool(1, $workerCount)
    $jobs = [Collections.Generic.List[object]]::new()
    try {
        $pool.Open()
        for ($worker = 0; $worker -lt $workerCount; $worker++) {
            [long] $startPiece = $worker * $piecesPerWorker
            [long] $endPiece = [Math]::Min($pieceCount, $startPiece + $piecesPerWorker)
            if ($startPiece -ge $endPiece) { break }
            $pipeline = [PowerShell]::Create()
            $pipeline.RunspacePool = $pool
            $job = [pscustomobject] @{ Pipeline = $pipeline; Handle = $null; Done = $false }
            $jobs.Add($job)
            [void] $pipeline.AddScript($workerCode.ToString()).AddArgument($Paths).AddArgument($Lengths).
                AddArgument($WriteTimes).AddArgument($PieceLength).AddArgument($startPiece).AddArgument($endPiece).
                AddArgument($TotalBytes).AddArgument($worker).AddArgument($progressBytes)
            $job.Handle = $pipeline.BeginInvoke()
        }
        $pieces = [IO.MemoryStream]::new()
        try {
            $pieces.SetLength($pieceCount * 20)
            $remainingJobs = $jobs.Count
            while ($remainingJobs) {
                foreach ($job in $jobs) {
                    if (!$job.Done -and $job.Handle.IsCompleted) {
                        $output = $job.Pipeline.EndInvoke($job.Handle)
                        if ($job.Pipeline.HadErrors) { throw ($job.Pipeline.Streams.Error | Out-String) }
                        if ($output.Count -ne 1) { throw 'Unexpected parallel hash result.' }
                        $result = $output[0]
                        $pieces.Position = $result.StartPiece * 20
                        $pieces.Write($result.Pieces, 0, $result.Pieces.Length)
                        $job.Done = $true
                        $remainingJobs--
                    }
                }
                if ($null -ne $progressBytes) {
                    [long] $doneBytes = 0
                    foreach ($value in $progressBytes.Values) { $doneBytes += $value }
                    Write-Progress -Activity 'Creating torrent' -Status "$doneBytes / $TotalBytes bytes ($workerCount workers)" `
                        -PercentComplete ([Math]::Min(100, 100.0 * $doneBytes / $TotalBytes))
                }
                if ($remainingJobs) { Start-Sleep -Milliseconds 100 }
            }
            return ,$pieces.ToArray()
        } finally { $pieces.Dispose() }
    } finally {
        foreach ($job in $jobs) {
            try { if ($null -ne $job.Handle -and !$job.Handle.IsCompleted) { $job.Pipeline.Stop() } }
            catch { Write-Verbose "Worker cleanup: $_" }
            finally { $job.Pipeline.Dispose() }
        }
        $pool.Dispose()
    }
}

# Read and verification functions live below; all modes use the same binary codec.
function Read-BencodeValue {
    param($State, [int] $Depth = 0)
    if ($Depth -gt 128 -or $State.Position -ge $State.Data.Length) { throw 'Invalid Bencode depth or unexpected end.' }
    $start = $State.Position
    $token = $State.Data[$State.Position++]
    if ($token -ge 48 -and $token -le 57) {
        while ($State.Position -lt $State.Data.Length -and $State.Data[$State.Position] -ne 58) {
            if ($State.Data[$State.Position] -lt 48 -or $State.Data[$State.Position] -gt 57) { throw 'Invalid byte string length.' }
            $State.Position++
        }
        if ($State.Position -ge $State.Data.Length) { throw 'Unterminated byte string length.' }
        $digits = [Text.Encoding]::ASCII.GetString($State.Data, $start, $State.Position - $start)
        if ($digits.Length -gt 1 -and $digits[0] -eq '0') { throw 'Noncanonical byte string length.' }
        [long] $length = 0
        if (![long]::TryParse($digits, [ref] $length)) { throw 'Byte string length overflow.' }
        $State.Position++
        if ($length -gt $State.Data.Length - $State.Position) { throw 'Truncated byte string.' }
        $bytes = [byte[]]::new([int] $length)
        [Array]::Copy($State.Data, $State.Position, $bytes, 0, [int] $length)
        $State.Position += [int] $length
        return ,$bytes
    }
    if ($token -eq 105) {
        $start = $State.Position
        while ($State.Position -lt $State.Data.Length -and $State.Data[$State.Position] -ne 101) { $State.Position++ }
        if ($State.Position -ge $State.Data.Length) { throw 'Unterminated integer.' }
        $digits = [Text.Encoding]::ASCII.GetString($State.Data, $start, $State.Position - $start)
        [long] $number = 0
        if ($digits -cnotmatch '^(0|-?[1-9][0-9]*)$' -or ![long]::TryParse($digits, [ref] $number)) { throw 'Invalid Bencode integer.' }
        $State.Position++
        return $number
    }
    if ($token -eq 108 -or $token -eq 100) {
        $list = [Collections.Generic.List[object]]::new()
        $dictionary = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
        while ($State.Position -lt $State.Data.Length -and $State.Data[$State.Position] -ne 101) {
            if ($token -eq 108) { $list.Add((Read-BencodeValue $State ($Depth + 1))) }
            else {
                $keyBytes = Read-BencodeValue $State ($Depth + 1)
                if ($keyBytes -isnot [byte[]]) { throw 'Dictionary key must be a byte string.' }
                $key = $State.Utf8.GetString($keyBytes)
                if ($dictionary.ContainsKey($key)) { throw 'Duplicate Bencode dictionary key.' }
                $valueStart = $State.Position
                $value = Read-BencodeValue $State ($Depth + 1)
                $dictionary.Add($key, $value)
                if ($Depth -eq 0 -and $key -ceq 'info') {
                    $State.InfoStart = $valueStart
                    $State.InfoLength = $State.Position - $valueStart
                }
            }
        }
        if ($State.Position -ge $State.Data.Length) { throw 'Unterminated Bencode container.' }
        $State.Position++
        if ($token -eq 108) { return ,$list }
        return ,$dictionary
    }
    throw 'Invalid Bencode token.'
}

function Get-TorrentText {
    param($Dictionary, [string] $Key, [switch] $Required)
    if (!$Dictionary.ContainsKey($Key)) {
        if ($Required) { throw "Missing torrent field: $Key" }
        return $null
    }
    if ($Dictionary[$Key] -isnot [byte[]]) { throw "Expected byte string: $Key" }
    return [Text.UTF8Encoding]::new($false, $true).GetString($Dictionary[$Key])
}

function Assert-TorrentComponent {
    param([string] $Name)
    if (!$Name -or $Name -in @('.', '..') -or $Name.IndexOfAny([char[]] '/\:') -ge 0 -or
        $Name.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0 -or
        ($IsWindows -and ($Name -match '[. ]$' -or $Name -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)'))) {
        throw 'Torrent contains an unsafe or unsupported path component.'
    }
}

function Read-Torrent {
    param([string] $TorrentPath)
    $resolved = (Get-Item -LiteralPath $TorrentPath -Force).FullName
    $state = [pscustomobject] @{
        Data = [IO.File]::ReadAllBytes($resolved); Position = 0
        InfoStart = -1; InfoLength = 0; Utf8 = [Text.UTF8Encoding]::new($false, $true)
    }
    $metadata = Read-BencodeValue $state
    if ($state.Position -ne $state.Data.Length -or $metadata -isnot [Collections.IDictionary] -or
        !$metadata.ContainsKey('info') -or $metadata['info'] -isnot [Collections.IDictionary]) {
        throw 'Invalid torrent root or trailing data.'
    }
    $info = $metadata['info']
    if ($info.ContainsKey('meta version')) { throw 'Only BitTorrent v1 torrents are supported; v2/hybrid torrents are not supported.' }
    $name = Get-TorrentText $info 'name' -Required
    Assert-TorrentComponent $name
    if (!$info.ContainsKey('piece length') -or $info['piece length'] -isnot [long] -or
        $info['piece length'] -le 0 -or $info['piece length'] -gt 134217728) { throw 'Invalid or unsupported piece length (maximum 128 MiB).' }
    if (!$info.ContainsKey('pieces') -or $info['pieces'] -isnot [byte[]] -or $info['pieces'].Length % 20) { throw 'Invalid pieces field.' }
    $files = [Collections.Generic.List[object]]::new()
    $multi = $info.ContainsKey('files')
    if ($multi -eq $info.ContainsKey('length')) { throw 'Torrent must contain either files or length.' }
    if ($multi) {
        if ($info['files'] -isnot [Collections.Generic.List[object]] -or !$info['files'].Count) { throw 'Invalid files list.' }
        $entries = $info['files']
    } else { $entries = @($info) }
    $comparer = if ($IsWindows) { [StringComparer]::OrdinalIgnoreCase } else { [StringComparer]::Ordinal }
    $seen = [Collections.Generic.HashSet[string]]::new($comparer)
    [long] $total = 0
    foreach ($entry in $entries) {
        if ($entry -isnot [Collections.IDictionary] -or !$entry.ContainsKey('length') -or
            $entry['length'] -isnot [long] -or $entry['length'] -lt 0) { throw 'Invalid file length.' }
        if ($multi) {
            if (!$entry.ContainsKey('path') -or $entry['path'] -isnot [Collections.Generic.List[object]] -or !$entry['path'].Count) { throw 'Invalid file path.' }
            $components = [Collections.Generic.List[string]]::new()
            foreach ($part in $entry['path']) {
                if ($part -isnot [byte[]]) { throw 'Invalid file path component.' }
                $component = $state.Utf8.GetString($part)
                Assert-TorrentComponent $component
                $components.Add($component)
            }
            $relative = [string]::Join([IO.Path]::DirectorySeparatorChar, $components)
        } else { $relative = $name }
        if (!$seen.Add($relative)) { throw 'Duplicate torrent file path.' }
        $files.Add([pscustomobject] @{ Path = $relative; Length = $entry['length']; Offset = $total })
        if ($entry['length'] -gt [long]::MaxValue - $total) { throw 'Total file length overflow.' }
        $total += $entry['length']
    }
    foreach ($file in $files) {
        $parent = [IO.Path]::GetDirectoryName($file.Path)
        while ($parent) {
            if ($seen.Contains($parent)) { throw 'Conflicting file and directory paths.' }
            $parent = [IO.Path]::GetDirectoryName($parent)
        }
    }
    $pieceLength = $info['piece length']
    [long] $expectedPieces = [long] [decimal]::Ceiling([decimal] $total / $pieceLength)
    if ($expectedPieces -ne $info['pieces'].Length / 20) { throw 'Piece count does not match total file length.' }
    $sha1 = [Security.Cryptography.SHA1]::Create()
    try { $hash = $sha1.ComputeHash($state.Data, $state.InfoStart, $state.InfoLength) } finally { $sha1.Dispose() }
    $trackers = [Collections.Generic.List[object]]::new()
    if ($metadata.ContainsKey('announce-list')) {
        if ($metadata['announce-list'] -isnot [Collections.Generic.List[object]]) { throw 'Invalid announce-list.' }
        foreach ($tier in $metadata['announce-list']) {
            if ($tier -isnot [Collections.Generic.List[object]]) { throw 'Invalid tracker tier.' }
            $urls = [Collections.Generic.List[string]]::new()
            foreach ($url in $tier) {
                if ($url -isnot [byte[]]) { throw 'Invalid tracker URL.' }
                $urls.Add($state.Utf8.GetString($url))
            }
            $trackers.Add($urls.ToArray())
        }
    }
    $privateValue = $false
    if ($info.ContainsKey('private')) {
        if ($info['private'] -isnot [long] -or $info['private'] -notin @(0, 1)) { throw 'Invalid private flag.' }
        $privateValue = $info['private'] -eq 1
    }
    $date = $null
    if ($metadata.ContainsKey('creation date')) {
        if ($metadata['creation date'] -isnot [long]) { throw 'Invalid creation date.' }
        $date = [DateTimeOffset]::FromUnixTimeSeconds($metadata['creation date'])
    }
    return [pscustomobject] @{
        Torrent = $resolved; Name = $name
        InfoHash = [BitConverter]::ToString($hash).Replace('-', '').ToLowerInvariant()
        Bytes = $total; PieceLength = $pieceLength; Pieces = $expectedPieces
        Private = $privateValue; Source = (Get-TorrentText $info 'source')
        Announce = (Get-TorrentText $metadata 'announce'); AnnounceList = $trackers.ToArray()
        Comment = (Get-TorrentText $metadata 'comment'); CreatedBy = (Get-TorrentText $metadata 'created by')
        CreationDate = $date; Encoding = (Get-TorrentText $metadata 'encoding')
        IsDirectory = $multi; Files = $files.ToArray(); PieceHashes = $info['pieces']
    }
}

function Show-Torrent {
    param($Torrent, [switch] $Tree)
    foreach ($key in @('Torrent', 'Name', 'InfoHash', 'Bytes', 'PieceLength', 'Pieces', 'Private', 'Source', 'Announce', 'Comment', 'CreatedBy', 'CreationDate', 'Encoding')) {
        Write-Host ('{0}: {1}' -f $key, $Torrent.$key)
    }
    $tierIndex = 0
    foreach ($tier in $Torrent.AnnounceList) {
        foreach ($url in $tier) { Write-Host "Tracker tier ${tierIndex}: $url" }
        $tierIndex++
    }
    Write-Host "Files: $($Torrent.Files.Count)"
    if (!$Tree) {
        foreach ($file in $Torrent.Files) { Write-Host "  $($file.Path) [$($file.Length) bytes]" }
    } else {
        if (!$Torrent.IsDirectory) {
            Write-Host "$($Torrent.Name) [$($Torrent.Bytes) bytes]"
            return
        }
        Write-Host "$($Torrent.Name)/"
        $shown = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        $treeFiles = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
        foreach ($file in $Torrent.Files) { $treeFiles.Add($file.Path, $file) }
        [string[]] $treePaths = @($treeFiles.Keys)
        [Array]::Sort($treePaths, [StringComparer]::Ordinal)
        foreach ($treePath in $treePaths) {
            $file = $treeFiles[$treePath]
            $parts = $file.Path.Split([IO.Path]::DirectorySeparatorChar)
            for ($i = 0; $i -lt $parts.Count; $i++) {
                $prefix = [string]::Join('/', $parts[0..$i])
                if ($shown.Add($prefix)) {
                    $suffix = if ($i -eq $parts.Count - 1) { " [$($file.Length) bytes]" } else { '/' }
                    Write-Host (('  ' * ($i + 1)) + $parts[$i] + $suffix)
                }
            }
        }
    }
}

function Test-Torrent {
    param($Torrent, [string] $ContentPath)
    $root = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ContentPath)
    $buffer = [byte[]]::new([int] $Torrent.PieceLength)
    $badPieces = [Collections.Generic.List[int]]::new()
    $fileStates = [Collections.Generic.List[object]]::new()
    $sha1 = [Security.Cryptography.SHA1]::Create()
    $filled = 0
    $pieceIndex = 0
    $unknown = $false
    try {
        foreach ($file in $Torrent.Files) {
            $fullPath = if ($Torrent.IsDirectory) { [IO.Path]::Combine($root, $file.Path) } else { $root }
            # Reject links in every ancestor before reading any content.
            $probe = $fullPath
            while ($probe) {
                if (Test-Path -LiteralPath $probe) {
                    $item = Get-Item -LiteralPath $probe -Force
                    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Verification through links/reparse points is not supported.' }
                }
                $probe = [IO.Path]::GetDirectoryName($probe)
            }
            $reason = $null
            $stream = $null
            $timestamp = $null
            try {
                if (![IO.File]::Exists($fullPath)) { $reason = 'Missing file' }
                else {
                    $stream = [IO.File]::Open($fullPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
                    $timestamp = [IO.File]::GetLastWriteTimeUtc($fullPath)
                    if ($stream.Length -ne $file.Length) { $reason = "Size mismatch (expected $($file.Length), actual $($stream.Length))" }
                }
                [long] $remaining = $file.Length
                while ($remaining -gt 0) {
                    $wanted = [int] [Math]::Min($remaining, $buffer.Length - $filled)
                    $count = 0
                    if ($null -ne $stream) { $count = $stream.Read($buffer, $filled, $wanted) }
                    if (!$count) {
                        # Unknown bytes occupy their declared positions so later pieces stay aligned.
                        [Array]::Clear($buffer, $filled, $wanted)
                        $unknown = $true
                        $count = $wanted
                        if (!$reason) { $reason = 'File ended before declared length' }
                    }
                    $filled += $count
                    $remaining -= $count
                    if ($filled -eq $buffer.Length -or ($file.Offset + $file.Length - $remaining -eq $Torrent.Bytes)) {
                        $hash = $sha1.ComputeHash($buffer, 0, $filled)
                        $matches = !$unknown
                        for ($i = 0; $i -lt 20; $i++) {
                            if ($hash[$i] -ne $Torrent.PieceHashes[$pieceIndex * 20 + $i]) { $matches = $false }
                        }
                        if (!$matches) { $badPieces.Add($pieceIndex) }
                        $pieceIndex++
                        $filled = 0
                        $unknown = $false
                        Write-Progress -Activity 'Verifying torrent' -Status "$pieceIndex / $($Torrent.Pieces) pieces" `
                            -PercentComplete (100.0 * $pieceIndex / $Torrent.Pieces)
                    }
                }
                if ($null -ne $stream -and [IO.File]::GetLastWriteTimeUtc($fullPath) -ne $timestamp) {
                    throw 'Source changed during verification.'
                }
            } finally { if ($null -ne $stream) { $stream.Dispose() } }
            $fileStates.Add([pscustomobject] @{ Path = $file.Path; Length = $file.Length; Offset = $file.Offset; Reason = $reason })
        }
        $badSet = [Collections.Generic.HashSet[int]]::new($badPieces)
        $failedFiles = [Collections.Generic.List[object]]::new()
        foreach ($file in $fileStates) {
            if ($file.Length -gt 0) {
                $first = [long] [decimal]::Floor([decimal] $file.Offset / $Torrent.PieceLength)
                $last = [long] [decimal]::Floor([decimal] ($file.Offset + $file.Length - 1) / $Torrent.PieceLength)
                for ($index = $first; $index -le $last; $index++) {
                    if ($badSet.Contains([int] $index)) {
                        if (!$file.Reason) { $file.Reason = 'Touches a failed piece' }
                        break
                    }
                }
            }
            if ($file.Reason) { $failedFiles.Add($file) }
        }
        return [pscustomobject] @{
            Torrent = $Torrent.Torrent; Target = $root; InfoHash = $Torrent.InfoHash
            IsValid = ($badPieces.Count -eq 0 -and $failedFiles.Count -eq 0)
            TotalPieces = $Torrent.Pieces; PassedPieces = $pieceIndex - $badPieces.Count
            FailedPieces = $badPieces.ToArray(); TotalFiles = $fileStates.Count
            PassedFiles = $fileStates.Count - $failedFiles.Count; FailedFiles = $failedFiles.ToArray()
        }
    } finally {
        $sha1.Dispose()
        Write-Progress -Activity 'Verifying torrent' -Completed
    }
}

if ($Mode -eq 'Auto') {
    if ($TargetPath) { $Mode = 'Verify' }
    elseif ([IO.Path]::GetExtension($Path) -ieq '.torrent') { $Mode = 'Info' }
    else { $Mode = 'Create' }
}
if ($TargetPath -and $Mode -ne 'Verify') { throw 'A second path is only supported in Verify mode.' }
if ($Mode -ne 'Create') {
    foreach ($parameterName in @('Output', 'PieceSize', 'Announce', 'Private', 'Comment', 'Source', 'NoDate', 'Force', 'Threads')) {
        if ($PSBoundParameters.ContainsKey($parameterName)) { throw "-$parameterName is only supported in Create mode." }
    }
    if ($Mode -eq 'Verify' -and [IO.Path]::GetExtension($TargetPath) -ieq '.torrent' -and
        [IO.Path]::GetExtension($Path) -ine '.torrent') {
        $Path, $TargetPath = $TargetPath, $Path
    }
    $torrent = Read-Torrent $Path
    if ($Mode -eq 'Info') {
        if ($PassThru) { $torrent } else { Show-Torrent $torrent -Tree:$PrintTree }
    } else {
        if (!$TargetPath) { throw 'Verify mode requires -TargetPath (the actual file or content directory).' }
        if ($PrintTree) { throw '-PrintTree is only supported in Info mode.' }
        $report = Test-Torrent $torrent $TargetPath
        if ($PassThru) { $report } else {
            Write-Host "Pieces: $($report.PassedPieces) passed, $($report.FailedPieces.Count) failed"
            Write-Host "Files: $($report.PassedFiles) passed, $($report.FailedFiles.Count) failed"
            foreach ($failure in $report.FailedFiles) { Write-Host "  $($failure.Path): $($failure.Reason)" }
            Write-Host "Valid: $($report.IsValid)"
        }
        if (!$report.IsValid) { throw 'Torrent verification failed.' }
    }
    return
}
if ($PrintTree) { throw '-PrintTree is only supported in Info mode.' }

$inputItem = Get-Item -LiteralPath $Path -Force
if ($inputItem -isnot [System.IO.FileSystemInfo]) { throw 'Path must be a filesystem path.' }
if ($inputItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
    throw 'Symbolic links and reparse points are not supported.'
}
$inputPath = $inputItem.FullName
$isDirectory = $inputItem -is [System.IO.DirectoryInfo]
if ([string]::IsNullOrEmpty($inputItem.Name)) { throw 'Use a named file or directory, not a filesystem root.' }
if (!$Output) { $Output = $inputPath.TrimEnd('\', '/') + '.torrent' }
$outputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Output)
$comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
if ($outputPath.Equals($inputPath, $comparison) -or
    ($isDirectory -and $outputPath.StartsWith(
        $inputPath.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar, $comparison))) {
    throw 'Output must be outside the source file or directory.'
}
$outputDirectory = [IO.Path]::GetDirectoryName($outputPath)
if (![IO.Directory]::Exists($outputDirectory)) { throw "Output directory does not exist: $outputDirectory" }
if ([IO.Directory]::Exists($outputPath)) { throw "Output is a directory: $outputPath" }
if ([IO.File]::Exists($outputPath) -and !$Force) { throw "Output already exists; use -Force to overwrite: $outputPath" }
foreach ($tracker in $Announce) {
    $uri = $null
    if (![Uri]::TryCreate($tracker, [UriKind]::Absolute, [ref] $uri) -or
        $uri.Scheme -notin @('http', 'https', 'udp') -or !$uri.Host) {
        throw 'Tracker URLs must be absolute http, https, or udp URLs.'
    }
}

$fileMap = [Collections.Generic.Dictionary[string, IO.FileInfo]]::new([StringComparer]::Ordinal)
if ($isDirectory) {
    # Explicit traversal checks directory links before descending into them.
    $pending = [Collections.Generic.Stack[IO.DirectoryInfo]]::new()
    $pending.Push($inputItem)
    while ($pending.Count) {
        foreach ($entry in $pending.Pop().EnumerateFileSystemInfos()) {
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Symbolic links and reparse points are not supported: $($entry.FullName)"
            }
            if ($entry -is [IO.DirectoryInfo]) { $pending.Push($entry) }
            else {
                $relative = [IO.Path]::GetRelativePath($inputPath, $entry.FullName)
                $fileMap.Add($relative, $entry)
            }
        }
    }
} else { $fileMap.Add($inputItem.Name, $inputItem) }
if (!$fileMap.Count) { throw 'The source directory contains no files.' }
[string[]] $relativePaths = @($fileMap.Keys)
[Array]::Sort($relativePaths, [StringComparer]::Ordinal)
[long] $totalBytes = 0
foreach ($relative in $relativePaths) { $totalBytes += $fileMap[$relative].Length }

$pieceLength = 1 -shl $PieceSize
$buffer = $null
if ($Threads -eq 1) { $buffer = [byte[]]::new($pieceLength) }
$pieces = [IO.MemoryStream]::new()
$sha1 = [Security.Cryptography.SHA1]::Create()
$infoBytes = [IO.MemoryStream]::new()
$tempPath = $null
try {
    if ($Threads -eq 1) {
        $filled = 0
        [long] $processed = 0
        $fileMetadata = [Collections.Generic.List[object]]::new()
        foreach ($relative in $relativePaths) {
            $file = $fileMap[$relative]
            [long] $expectedLength = $file.Length
            $expectedWriteTime = $file.LastWriteTimeUtc
            # FileShare.Read prevents writes/deletions while this file is open on Windows.
            $stream = [IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try {
                if ($stream.Length -ne $expectedLength) { throw "Source changed: $relative" }
                [long] $readLength = 0
                while (($count = $stream.Read($buffer, $filled, $pieceLength - $filled)) -gt 0) {
                    $filled += $count
                    $processed += $count
                    $readLength += $count
                    if ($filled -eq $pieceLength) {
                        $hash = $sha1.ComputeHash($buffer, 0, $filled)
                        $pieces.Write($hash, 0, $hash.Length)
                        $filled = 0
                    }
                    Write-Progress -Activity 'Creating torrent' -Status "$processed / $totalBytes bytes" `
                        -PercentComplete $(if ($totalBytes) { [Math]::Min(100, 100.0 * $processed / $totalBytes) } else { 100 })
                }
                $file.Refresh()
                if ($readLength -ne $expectedLength -or $file.LastWriteTimeUtc -ne $expectedWriteTime) {
                    throw "Source changed while hashing: $relative"
                }
            } finally { $stream.Dispose() }
            $fileMetadata.Add(@{
                length = $expectedLength
                path = [string[]] $relative.Split([IO.Path]::DirectorySeparatorChar)
            })
        }
        # A partial piece continues across file boundaries; empty content has no pieces.
        if ($filled -gt 0) {
            $hash = $sha1.ComputeHash($buffer, 0, $filled)
            $pieces.Write($hash, 0, $hash.Length)
        }
    } else {
        $orderedPaths = [Collections.Generic.List[string]]::new()
        $lengths = [Collections.Generic.List[long]]::new()
        $writeTimes = [Collections.Generic.List[DateTime]]::new()
        $fileMetadata = [Collections.Generic.List[object]]::new()
        foreach ($relative in $relativePaths) {
            $file = $fileMap[$relative]
            $orderedPaths.Add($file.FullName)
            $lengths.Add($file.Length)
            $writeTimes.Add($file.LastWriteTimeUtc)
            $fileMetadata.Add(@{ length = $file.Length; path = [string[]] $relative.Split([IO.Path]::DirectorySeparatorChar) })
        }
        $hashes = Invoke-ParallelCreationHash $orderedPaths.ToArray() $lengths.ToArray() $writeTimes.ToArray() $pieceLength $totalBytes $Threads
        foreach ($relative in $relativePaths) {
            $file = $fileMap[$relative]
            $expectedLength = $file.Length
            $expectedWriteTime = $file.LastWriteTimeUtc
            $file.Refresh()
            if (!$file.Exists -or $file.Length -ne $expectedLength -or $file.LastWriteTimeUtc -ne $expectedWriteTime) {
                throw "Source changed while hashing: $relative"
            }
        }
        $pieces.Write($hashes, 0, $hashes.Length)
    }
    $info = @{
        name = $inputItem.Name
        'piece length' = $pieceLength
        pieces = $pieces.ToArray()
    }
    if ($isDirectory) { $info.files = $fileMetadata }
    else { $info.length = $totalBytes }
    if ($Private) { $info.private = 1 }
    if ($PSBoundParameters.ContainsKey('Source')) { $info.source = $Source }
    $metadata = @{ info = $info; 'created by' = 'TorrentUtils.ps1'; encoding = 'UTF-8' }
    if (!$NoDate) { $metadata['creation date'] = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() }
    if ($PSBoundParameters.ContainsKey('Comment')) { $metadata.comment = $Comment }
    if ($Announce.Count) {
        $metadata.announce = $Announce[0]
        $tiers = [Collections.Generic.List[object]]::new()
        foreach ($tracker in $Announce) { $tiers.Add([string[]] @($tracker)) }
        $metadata['announce-list'] = $tiers
    }
    Write-Bencode $infoBytes $info
    $infoHash = [BitConverter]::ToString($sha1.ComputeHash($infoBytes.ToArray())).Replace('-', '').ToLowerInvariant()
    # Stage beside the output, then rename; a failure never leaves a partial torrent.
    $tempPath = [IO.Path]::Combine($outputDirectory, [IO.Path]::GetRandomFileName())
    $outputStream = [IO.File]::Open($tempPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { Write-Bencode $outputStream $metadata } finally { $outputStream.Dispose() }
    [IO.File]::Move($tempPath, $outputPath, [bool] $Force)
    $tempPath = $null
    [pscustomobject] @{
        Torrent = $outputPath
        InfoHash = $infoHash
        Files = $fileMap.Count
        Bytes = $totalBytes
        PieceLength = $pieceLength
        Pieces = [long] ($pieces.Length / 20)
    }
} finally {
    Write-Progress -Activity 'Creating torrent' -Completed
    $sha1.Dispose()
    $pieces.Dispose()
    $infoBytes.Dispose()
    if ($tempPath -and [IO.File]::Exists($tempPath)) { [IO.File]::Delete($tempPath) }
}
