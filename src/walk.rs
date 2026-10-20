use std::cmp;
use std::fs::metadata;
use std::path::{MAIN_SEPARATOR, Path};

use natord::compare_ignore_case;
use walkdir::WalkDir;

use crate::error::{TrError, TrResult};
use crate::tr_file::TrFile;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WalkMode {
    #[default]
    Default,
    Alphabetical,
    BreadthFirstAlphabetical, // tu like
    BreadthFirstLevel,        // qb like
    FileSize,
}

pub(crate) struct WalkedFiles {
    pub(crate) name: String,
    pub(crate) single_file: bool,
    pub(crate) files: Vec<TrFile>,
}

fn compare_paths(a: &[String], b: &[String], level_first: bool) -> cmp::Ordering {
    a.iter()
        .zip(b.iter())
        .enumerate()
        .find_map(|(depth, (seg_a, seg_b))| {
            if level_first {
                match (depth + 1 == a.len(), depth + 1 == b.len()) {
                    (true, false) => return Some(cmp::Ordering::Less),
                    (false, true) => return Some(cmp::Ordering::Greater),
                    _ => {}
                }
            }
            let cmp_res = compare_ignore_case(seg_a, seg_b);
            (cmp_res != cmp::Ordering::Equal).then_some(cmp_res)
        })
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

pub(crate) fn walk_target(target_path: &Path, walk_mode: WalkMode) -> TrResult<WalkedFiles> {
    let base_path = target_path;
    let name = base_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            TrError::InvalidPath(format!(
                "Invalid file name in path: {}",
                base_path.display()
            ))
        })?;
    let mut single_file = false;

    let base_metadata = metadata(base_path)?;
    let mut tr_files: Vec<TrFile> = Vec::new();

    if base_metadata.is_file() {
        single_file = true;
        tr_files.push(TrFile::new(base_metadata.len() as usize, Vec::new()));
    } else if base_metadata.is_dir() {
        for entry in WalkDir::new(base_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let entry_metadata = metadata(entry.path())?;
                let relative_path = entry
                    .path()
                    .strip_prefix(base_path)
                    .map_err(|_| {
                        TrError::InvalidPath(String::from("Failed to create relative path"))
                    })?
                    .to_str()
                    .ok_or_else(|| {
                        TrError::InvalidPath(String::from("Path contains invalid UTF-8"))
                    })?
                    .split(MAIN_SEPARATOR)
                    .map(str::to_owned)
                    .collect();

                tr_files.push(TrFile::new(entry_metadata.len() as usize, relative_path));
            }
        }
    } else {
        return Err(TrError::InvalidPath(String::from(
            "Target path is neither a file nor a directory",
        )));
    }

    match walk_mode {
        WalkMode::Default => {}
        WalkMode::Alphabetical => {
            tr_files.sort_by(|a, b| a.path().cmp(b.path()));
        }
        WalkMode::BreadthFirstAlphabetical => {
            tr_files.sort_by(|a, b| compare_paths(a.path(), b.path(), false));
        }
        WalkMode::BreadthFirstLevel => {
            tr_files.sort_by(|a, b| compare_paths(a.path(), b.path(), true));
        }
        WalkMode::FileSize => {
            tr_files.sort_by_key(|file| cmp::Reverse(file.length()));
        }
    }

    Ok(WalkedFiles {
        name: name.to_string(),
        single_file,
        files: tr_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments
            .iter()
            .map(|segment| (*segment).to_string())
            .collect()
    }

    #[test]
    fn breadth_first_alphabetical_compares_path_segments() {
        assert_eq!(
            compare_paths(&path(&["a", "z"]), &path(&["b"]), false),
            cmp::Ordering::Less
        );
        assert_eq!(
            compare_paths(&path(&["dir"]), &path(&["dir", "a"]), false),
            cmp::Ordering::Less
        );
    }

    #[test]
    fn breadth_first_level_prioritizes_shallower_entries() {
        assert_eq!(
            compare_paths(&path(&["a", "z"]), &path(&["b"]), true),
            cmp::Ordering::Greater
        );
        assert_eq!(
            compare_paths(&path(&["b"]), &path(&["a", "z"]), true),
            cmp::Ordering::Less
        );
        assert_eq!(
            compare_paths(&path(&["dir", "x"]), &path(&["dir", "sub", "y"]), true),
            cmp::Ordering::Less
        );
        assert_eq!(
            compare_paths(&path(&["dir", "a"]), &path(&["dir", "b"]), true),
            cmp::Ordering::Less
        );
    }
}
