use std::collections::HashMap;
use std::fmt::{self, Display, Formatter, Result as FmtResult, Write};

use chrono::{Local, TimeZone};
use natord::compare_ignore_case;
use torrent_utils::{Torrent, TrFile};

use crate::cli_output::{StyledLine, StyledSpan};

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

fn labeled_line(label: impl Into<String>, value: impl Into<String>) -> StyledLine {
    vec![StyledSpan::label(label), StyledSpan::plain(value)]
}

pub(crate) fn torrent_info(torrent: &Torrent) -> Vec<StyledLine> {
    let mut lines: Vec<StyledLine> = vec![vec![StyledSpan::label("Torrent Info:")]];

    let Some(info) = torrent.get_info() else {
        lines.push(vec![StyledSpan::plain("  [No torrent info available]")]);
        return lines;
    };

    if let Some(name) = info.name() {
        lines.push(labeled_line("  Name:", format!(" {name}")));
    }

    if let Some(announce_list) = torrent.announce_list() {
        lines.push(vec![StyledSpan::label("  Announce List:")]);
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
                    lines.push(labeled_line(
                        format!("    Tier {tier_str}:"),
                        format!(" {url}"),
                    ));
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
            lines.push(vec![StyledSpan::plain(format!(
                "    Truncated at {MAX_DISPLAYED_ANNOUNCES} announces..."
            ))]);
        }
    }

    if let Some(comment) = torrent.comment() {
        lines.push(labeled_line("  Comment:", format!(" {comment}")));
    }
    if let Some(created_by) = torrent.created_by() {
        lines.push(labeled_line("  Created by:", format!(" {created_by}")));
    }
    if let Some(date) = torrent.creation_date() {
        let value = if let Some(dt) = Local.timestamp_opt(date, 0).single() {
            format!(" {date} [{}]", dt.format("%Y-%m-%d %H:%M:%S"))
        } else {
            format!(" {date}")
        };
        lines.push(labeled_line("  Creation date:", value));
    }
    if let Some(encoding) = torrent.encoding() {
        lines.push(labeled_line("  Encoding:", format!(" {encoding}")));
    }
    if let Some(hash) = torrent.hash() {
        lines.push(labeled_line("  Hash:", format!(" {hash}")));
    }

    lines.push(labeled_line(
        "  Private:",
        format!(" {}", info.is_private()),
    ));
    if let Some(source) = info.source() {
        lines.push(labeled_line("  Source:", format!(" {source}")));
    }

    lines.push(labeled_line(
        "  Piece length:",
        format!(
            " {} [{}]",
            info.piece_length(),
            human_size(info.piece_length())
        ),
    ));

    let total_length = if let Some(files) = info.files() {
        files.iter().map(|f| f.length()).sum()
    } else {
        info.length().unwrap_or_default()
    };
    lines.push(labeled_line(
        "  Length:",
        format!(" {} [{}]", total_length, human_size(total_length)),
    ));

    let piece_count = if info.piece_length() > 0 {
        total_length.div_ceil(info.piece_length())
    } else {
        0
    };
    lines.push(labeled_line("  Pieces:", format!(" {piece_count}")));

    if let Some(files) = info.files() {
        lines.push(labeled_line("  Files:", format!("  {}", files.len())));
        lines.push(vec![StyledSpan::label("  RelPath [Length]:")]);
        let mut shown = 0;
        let mut truncated = false;
        for file in files {
            if shown < MAX_DISPLAYED_FILES {
                let path_str = file.path().join("/");
                lines.push(vec![StyledSpan::plain(format!(
                    "    - {path_str} [{} ({})]",
                    file.length(),
                    human_size(file.length())
                ))]);
                shown += 1;
            } else {
                truncated = true;
                break;
            }
        }
        if truncated {
            lines.push(vec![StyledSpan::plain(format!(
                "    Truncated at {MAX_DISPLAYED_FILES} files..."
            ))]);
        }
    }

    lines
}

pub(crate) fn file_tree(torrent: &Torrent) -> String {
    let mut output = String::new();
    write_file_tree(&mut output, torrent).expect("writing to a String cannot fail");
    output
}

fn write_file_tree(output: &mut impl Write, torrent: &Torrent) -> fmt::Result {
    match torrent.get_info() {
        Some(info) => {
            if let Some(name) = info.name() {
                writeln!(output, "{name}")?;
            }
            if let Some(files) = info.files() {
                write!(output, "{}", FileTree::build(files))?;
            } else if let Some(length) = info.length() {
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
            root.insert_path(f.path(), f.length());
        }
        root
    }

    fn fmt_children(&self, f: &mut Formatter<'_>, prefix: &str) -> FmtResult {
        let mut names: Vec<&String> = self.children.keys().collect();
        names.sort_by(|a, b| compare_ignore_case(a, b));

        for (idx, name) in names.iter().enumerate() {
            let last = idx == names.len() - 1;
            let child = self.children.get(*name).expect("tree child exists");
            child.fmt_branch(f, prefix, last)?;
        }
        Ok(())
    }

    fn fmt_tree(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.fmt_children(f, "")
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

        let new_prefix = format!("{prefix}{child_prefix}");
        self.fmt_children(f, &new_prefix)
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
    use crate::cli_output::OutputStyle;

    #[test]
    fn torrent_info_uses_structured_label_spans() {
        let encoded = b"d13:announce-listll15:https://exampleee4:infod6:lengthi3e4:name8:test.bin12:piece lengthi16384e6:pieces20:....................ee";
        let torrent = Torrent::from_bytes(encoded).expect("valid torrent");
        let lines = torrent_info(&torrent);

        assert_eq!(lines[0][0].text, "Torrent Info:");
        assert_eq!(lines[0][0].style, OutputStyle::Label);

        let tier = lines
            .iter()
            .find(|line| line[0].text == "    Tier 0:")
            .expect("tier line");
        assert_eq!(tier[0].style, OutputStyle::Label);
        assert_eq!(tier[1].text, " https://example");
        assert_eq!(tier[1].style, OutputStyle::Plain);

        let name = lines
            .iter()
            .find(|line| line[0].text == "  Name:")
            .expect("name line");
        assert_eq!(name[0].style, OutputStyle::Label);
        assert_eq!(name[1].text, " test.bin");
        assert_eq!(name[1].style, OutputStyle::Plain);
    }

    #[test]
    fn file_tree_and_human_sizes_are_deterministic() {
        let files = vec![
            TrFile::new(1024, vec![String::from("dir"), String::from("b.bin")]),
            TrFile::new(3, vec![String::from("a.txt")]),
        ];

        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1536), "1.50 KiB");
        assert_eq!(
            FileTree::build(&files).to_string(),
            "├── a.txt (3 [3 B])\n└── dir\n    └── b.bin (1024 [1 KiB])\n"
        );
    }
}
