use std::collections::HashMap;
use std::fmt::{self, Display, Formatter, Result as FmtResult, Write};

use chrono::{Local, TimeZone};
use natord::compare_ignore_case;
use torrent_utils::{Torrent, TrFile};

const MAX_DISPLAYED_ANNOUNCES: usize = 20;
const MAX_DISPLAYED_FILES: usize = 100;

pub(crate) fn human_size(bytes: usize) -> String {
    const UNITS: &[(usize, &str)] = &[
        (1024 * 1024 * 1024, "GiB"),
        (1024 * 1024, "MiB"),
        (1024, "KiB"),
    ];

    for &(unit_size, unit_name) in UNITS {
        if bytes >= unit_size {
            return if bytes.is_multiple_of(unit_size) {
                format!("{} {}", bytes / unit_size, unit_name)
            } else {
                let value = bytes as f64 / unit_size as f64;
                format!("{value:.2} {unit_name}")
            };
        }
    }

    format!("{bytes} B")
}

pub(crate) fn torrent_info(torrent: &Torrent) -> String {
    let mut output = String::new();
    write_torrent_info(&mut output, torrent).expect("writing to a String cannot fail");
    output
}

fn write_torrent_info(output: &mut impl Write, torrent: &Torrent) -> fmt::Result {
    writeln!(output, "Torrent Info:")?;

    let Some(info) = torrent.get_info() else {
        writeln!(output, "  [No torrent info available]")?;
        return Ok(());
    };

    if let Some(name) = &info.name {
        writeln!(output, "  Name: {name}")?;
    }

    if let Some(announce_list) = torrent.announce_list() {
        writeln!(output, "  Announce List:")?;
        let pad = if announce_list.len() >= 10 { 2 } else { 1 };
        let mut shown = 0;
        let mut truncated = false;
        for (tier_id, tier) in announce_list.iter().enumerate() {
            let tier_str = if pad == 2 {
                format!("{tier_id:02}")
            } else {
                format!("{tier_id}")
            };
            for url in tier {
                if shown < MAX_DISPLAYED_ANNOUNCES {
                    writeln!(output, "    Tier {tier_str}: {url}")?;
                    shown += 1;
                } else {
                    truncated = true;
                    break;
                }
            }
            if truncated {
                break;
            }
        }
        if truncated {
            writeln!(
                output,
                "    Truncated at {MAX_DISPLAYED_ANNOUNCES} announces..."
            )?;
        }
    }

    if let Some(comment) = torrent.comment() {
        writeln!(output, "  Comment: {comment}")?;
    }
    if let Some(created_by) = torrent.created_by() {
        writeln!(output, "  Created by: {created_by}")?;
    }
    if let Some(date) = torrent.creation_date() {
        if let Some(dt) = Local.timestamp_opt(date, 0).single() {
            let dt = dt.format("%Y-%m-%d %H:%M:%S");
            writeln!(output, "  Creation date: {date} [{dt}]")?;
        } else {
            writeln!(output, "  Creation date: {date}")?;
        }
    }
    if let Some(encoding) = torrent.encoding() {
        writeln!(output, "  Encoding: {encoding}")?;
    }
    if let Some(hash) = torrent.hash() {
        writeln!(output, "  Hash: {hash}")?;
    }

    writeln!(output, "  Private: {}", info.private)?;
    if let Some(source) = &info.source {
        writeln!(output, "  Source: {source}")?;
    }

    writeln!(
        output,
        "  Piece length: {} [{}]",
        info.piece_length,
        human_size(info.piece_length)
    )?;

    let total_length = if let Some(files) = &info.files {
        files.iter().map(|f| f.length).sum()
    } else {
        info.length.unwrap_or_default()
    };
    writeln!(
        output,
        "  Length: {} [{}]",
        total_length,
        human_size(total_length)
    )?;

    writeln!(
        output,
        "  Pieces: {}",
        if info.piece_length > 0 {
            total_length.div_ceil(info.piece_length)
        } else {
            0
        }
    )?;

    if let Some(files) = &info.files {
        writeln!(output, "  Files:  {}", files.len())?;
        writeln!(output, "  RelPath [Length]:")?;
        let mut shown = 0;
        let mut truncated = false;
        for file in files {
            if shown < MAX_DISPLAYED_FILES {
                let path_str = file.path.join("/");
                writeln!(
                    output,
                    "    - {path_str} [{} ({})]",
                    file.length,
                    human_size(file.length)
                )?;
                shown += 1;
            } else {
                truncated = true;
                break;
            }
        }
        if truncated {
            writeln!(output, "    Truncated at {MAX_DISPLAYED_FILES} files...")?;
        }
    }

    Ok(())
}

pub(crate) fn file_tree(torrent: &Torrent) -> String {
    let mut output = String::new();
    write_file_tree(&mut output, torrent).expect("writing to a String cannot fail");
    output
}

fn write_file_tree(output: &mut impl Write, torrent: &Torrent) -> fmt::Result {
    match torrent.get_info() {
        Some(info) => {
            if let Some(name) = &info.name {
                writeln!(output, "{name}")?;
            }
            if let Some(files) = &info.files {
                write!(output, "{}", FileTree::build(files))?;
            } else if let Some(length) = info.length {
                writeln!(
                    output,
                    "  [Single file, {} ({})]",
                    length,
                    human_size(length)
                )?;
            } else {
                writeln!(output, "  [No files information available]")?;
            }
        }
        None => writeln!(output, "[No torrent info available]")?,
    }
    Ok(())
}

#[derive(Debug)]
struct FileTree {
    name: String,
    length: Option<usize>, // None -> dir, Some(size) -> file
    children: HashMap<String, FileTree>,
}

impl FileTree {
    fn new_dir(name: &str) -> Self {
        FileTree {
            name: name.into(),
            length: None,
            children: HashMap::new(),
        }
    }

    fn new_file(name: &str, size: usize) -> Self {
        FileTree {
            name: name.into(),
            length: Some(size),
            children: HashMap::new(),
        }
    }

    fn insert_path(&mut self, segments: &[String], size: usize) {
        if segments.is_empty() {
            return;
        }
        if segments.len() == 1 {
            self.children
                .entry(segments[0].clone())
                .and_modify(|n| {
                    n.length = Some(size);
                })
                .or_insert_with(|| FileTree::new_file(&segments[0], size));
        } else {
            let dir = self
                .children
                .entry(segments[0].clone())
                .or_insert_with(|| FileTree::new_dir(&segments[0]));
            dir.insert_path(&segments[1..], size);
        }
    }

    fn build(files: &[TrFile]) -> FileTree {
        let mut root = FileTree::new_dir("");
        for f in files {
            root.insert_path(&f.path, f.length);
        }
        root
    }

    fn fmt_tree(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| compare_ignore_case(a, b));

        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).expect("tree child exists");
            child.fmt_branch(f, "", last)?;
        }
        Ok(())
    }

    fn fmt_branch(&self, f: &mut Formatter<'_>, prefix: &str, is_last: bool) -> FmtResult {
        let (connector, child_prefix) = if is_last {
            ("└── ", "    ")
        } else {
            ("├── ", "│   ")
        };

        match self.length {
            Some(sz) => writeln!(
                f,
                "{prefix}{connector}{} ({sz} [{}])",
                self.name,
                human_size(sz)
            )?,
            None => writeln!(f, "{prefix}{connector}{}", self.name)?,
        }

        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| compare_ignore_case(a, b));

        let new_prefix = format!("{prefix}{child_prefix}");
        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).expect("tree child exists");
            child.fmt_branch(f, &new_prefix, last)?;
        }
        Ok(())
    }
}

impl Display for FileTree {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.fmt_tree(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_tree_and_human_sizes_are_deterministic() {
        let files = vec![
            TrFile {
                length: 1024,
                path: vec![String::from("dir"), String::from("b.bin")],
            },
            TrFile {
                length: 3,
                path: vec![String::from("a.txt")],
            },
        ];

        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1536), "1.50 KiB");
        assert_eq!(
            FileTree::build(&files).to_string(),
            "├── a.txt (3 [3 B])\n└── dir\n    └── b.bin (1024 [1 KiB])\n"
        );
    }
}
