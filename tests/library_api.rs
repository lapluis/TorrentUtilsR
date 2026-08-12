use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use torrent_utils::{CreateOptions, FileTree, Torrent, TrFile, VerificationOptions, WalkMode};

fn temp_test_dir(name: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "torrent_utils_{name}_{}_{}",
        std::process::id(),
        unique
    ))
}

#[test]
fn creates_round_trips_and_verifies_a_single_file() {
    let test_dir = temp_test_dir("round_trip");
    fs::create_dir(&test_dir).expect("create test directory");
    let target = test_dir.join("payload.bin");
    fs::write(&target, b"abcdefghij").expect("write test payload");

    let options = CreateOptions {
        piece_length: 4,
        private: true,
        n_jobs: 2,
        walk_mode: WalkMode::Default,
        source: Some(String::from("library-test")),
    };
    let mut torrent = Torrent::new(None, None, None, None, None, Some(String::from("UTF-8")));
    torrent
        .create_torrent(&target, &options, None)
        .expect("create torrent metadata");

    let encoded = torrent.to_bytes().expect("serialize torrent");
    let parsed = Torrent::from_bytes(&encoded).expect("parse serialized torrent");
    let report = parsed
        .get_info()
        .expect("torrent has info")
        .verify(&target, &VerificationOptions { n_jobs: 2 }, None)
        .expect("verify payload");

    assert_eq!(report.total_pieces, 3);
    assert_eq!(report.total_files, 1);
    assert!(report.is_ok());

    fs::write(&target, b"abcdxfghij").expect("corrupt test payload");
    let report = parsed
        .get_info()
        .expect("torrent has info")
        .verify(&target, &VerificationOptions { n_jobs: 1 }, None)
        .expect("verify corrupted payload");
    assert!(!report.is_ok());
    assert_eq!(report.failed_files[0].path, "payload.bin");

    fs::remove_dir_all(&test_dir).expect("remove test directory");
}

#[test]
fn file_tree_is_formatted_without_writing_to_stdout() {
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

    let rendered = FileTree::build(&files).to_string();

    assert_eq!(
        rendered,
        "├── a.txt (3 [3 B])\n└── dir\n    └── b.bin (1024 [1 KiB])\n"
    );
}
