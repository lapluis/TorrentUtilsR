use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use torrent_utils::{
    CreateOptions, FileTree, ProgressReporter, Torrent, TrError, TrFile, TrInfo,
    VerificationOptions, WalkMode, human_size,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "torrent_utils_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn create_options(piece_length: usize) -> CreateOptions {
    CreateOptions {
        piece_length,
        private: true,
        n_jobs: 2,
        walk_mode: WalkMode::Alphabetical,
        source: Some(String::from("library-test")),
    }
}

fn empty_torrent() -> Torrent {
    Torrent::new(None, None, None, None, None, Some(String::from("UTF-8")))
}

#[test]
fn creates_round_trips_and_verifies_a_single_file() {
    let test_dir = TestDir::new("round_trip");
    let target = test_dir.path().join("payload.bin");
    fs::write(&target, b"abcdefghij").expect("write test payload");

    let mut torrent = empty_torrent();
    torrent
        .create_torrent(&target, &create_options(4), None)
        .expect("create torrent metadata");

    let encoded = torrent.to_bytes().expect("serialize torrent");
    let parsed = Torrent::from_bytes(&encoded).expect("parse serialized torrent");
    let report = parsed
        .get_info()
        .expect("torrent has info")
        .verify(&target, &VerificationOptions { n_jobs: 2 }, None)
        .expect("verify payload");

    assert_eq!(report.total_pieces, 3);
    assert_eq!(report.passed_pieces(), 3);
    assert_eq!(report.total_files, 1);
    assert_eq!(report.passed_files(), 1);
    assert!(report.is_ok());

    fs::write(&target, b"abcdxfghij").expect("corrupt test payload");
    let report = parsed
        .get_info()
        .expect("torrent has info")
        .verify(&target, &VerificationOptions { n_jobs: 1 }, None)
        .expect("verify corrupted payload");
    assert!(!report.is_ok());
    assert_eq!(report.failed_files.len(), 1);
    assert_eq!(report.failed_files[0].path, "payload.bin");
    assert!(!report.failed_files[0].missing_or_size_mismatch);
}

#[test]
fn empty_file_has_a_pieces_field_and_round_trips() {
    let test_dir = TestDir::new("empty_file");
    let target = test_dir.path().join("empty.bin");
    fs::write(&target, []).expect("write empty payload");

    let mut torrent = empty_torrent();
    torrent
        .create_torrent(&target, &create_options(16 * 1024), None)
        .expect("create empty torrent");
    let encoded = torrent.to_bytes().expect("serialize empty torrent");

    assert!(
        encoded
            .windows(b"6:pieces0:".len())
            .any(|window| window == b"6:pieces0:")
    );

    let parsed = Torrent::from_bytes(&encoded).expect("parse empty torrent");
    let report = parsed
        .get_info()
        .expect("torrent has info")
        .verify(&target, &VerificationOptions::default(), None)
        .expect("verify empty payload");
    assert_eq!(report.total_pieces, 0);
    assert!(report.is_ok());
}

#[test]
fn verifies_multi_file_hash_and_metadata_failures() {
    let test_dir = TestDir::new("multi_file");
    let target = test_dir.path().join("payload");
    fs::create_dir(&target).expect("create payload directory");
    fs::create_dir(target.join("nested")).expect("create nested directory");
    fs::write(target.join("a.bin"), b"abcdefgh").expect("write first file");
    fs::write(target.join("nested").join("b.bin"), b"ijklmnop").expect("write second file");

    let mut torrent = empty_torrent();
    torrent
        .create_torrent(&target, &create_options(4), None)
        .expect("create multi-file torrent");
    let info = torrent.get_info().expect("torrent has info");
    let files = info.files.as_ref().expect("multi-file torrent has files");
    assert_eq!(files[0].path, ["a.bin"]);
    assert_eq!(files[1].path, ["nested", "b.bin"]);

    let report = info
        .verify(&target, &VerificationOptions { n_jobs: 2 }, None)
        .expect("verify intact directory");
    assert!(report.is_ok());

    fs::write(target.join("a.bin"), b"abcdxfgh").expect("corrupt first file");
    let report = info
        .verify(&target, &VerificationOptions::default(), None)
        .expect("verify hash mismatch");
    assert_eq!(report.failed_files.len(), 1);
    assert_eq!(report.failed_files[0].path, "a.bin");
    assert!(!report.failed_files[0].missing_or_size_mismatch);

    fs::write(target.join("a.bin"), b"abcdefgh").expect("restore first file");
    fs::remove_file(target.join("nested").join("b.bin")).expect("remove second file");
    let report = info
        .verify(&target, &VerificationOptions::default(), None)
        .expect("verify missing file");
    assert_eq!(report.failed_files.len(), 1);
    assert_eq!(report.failed_files[0].path, "nested/b.bin");
    assert!(report.failed_files[0].missing_or_size_mismatch);
}

#[test]
fn chunked_hashing_preserves_cross_file_piece_hashes() {
    let test_dir = TestDir::new("cross_file_pieces");
    let directory = test_dir.path().join("payload");
    let single_file = test_dir.path().join("payload.bin");
    let payload: Vec<u8> = (0..41).collect();

    fs::create_dir(&directory).expect("create payload directory");
    fs::write(directory.join("a.bin"), &payload[..13]).expect("write first file");
    fs::write(directory.join("b.bin"), &payload[13..]).expect("write second file");
    fs::write(&single_file, &payload).expect("write equivalent single file");

    let options = create_options(4);
    let mut multi_file_torrent = empty_torrent();
    multi_file_torrent
        .create_torrent(&directory, &options, None)
        .expect("create multi-file torrent");
    let mut single_file_torrent = empty_torrent();
    single_file_torrent
        .create_torrent(&single_file, &options, None)
        .expect("create single-file torrent");

    assert_eq!(
        multi_file_torrent
            .get_info()
            .expect("multi-file torrent has info")
            .pieces,
        single_file_torrent
            .get_info()
            .expect("single-file torrent has info")
            .pieces
    );
}

#[derive(Default)]
struct RecordingProgress {
    total: AtomicUsize,
    advanced: AtomicUsize,
    finished: AtomicUsize,
}

impl ProgressReporter for RecordingProgress {
    fn begin(&self, total: usize) {
        self.total.store(total, Ordering::SeqCst);
    }

    fn advance(&self, delta: usize) {
        self.advanced.fetch_add(delta, Ordering::SeqCst);
    }

    fn finish(&self) {
        self.finished.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn reports_progress_for_every_piece() {
    let test_dir = TestDir::new("progress");
    let target = test_dir.path().join("payload.bin");
    fs::write(&target, b"abcdefghij").expect("write payload");
    let progress = RecordingProgress::default();

    let mut torrent = empty_torrent();
    torrent
        .create_torrent(&target, &create_options(4), Some(&progress))
        .expect("create torrent");

    assert_eq!(progress.total.load(Ordering::SeqCst), 3);
    assert_eq!(progress.advanced.load(Ordering::SeqCst), 3);
    assert_eq!(progress.finished.load(Ordering::SeqCst), 1);
}

#[test]
fn rejects_invalid_options_and_inconsistent_piece_data() {
    let test_dir = TestDir::new("invalid_options");
    let target = test_dir.path().join("payload.bin");
    fs::write(&target, b"abc").expect("write payload");

    let mut torrent = empty_torrent();
    let error = torrent
        .create_torrent(&target, &create_options(0), None)
        .expect_err("zero piece length must fail");
    assert!(matches!(error, TrError::InvalidConfig(_)));

    let mut options = create_options(4);
    options.n_jobs = 0;
    let error = torrent
        .create_torrent(&target, &options, None)
        .expect_err("zero jobs must fail");
    assert!(matches!(error, TrError::InvalidConfig(_)));

    let invalid_info = TrInfo {
        files: None,
        length: Some(3),
        name: Some(String::from("payload.bin")),
        piece_length: 2,
        pieces: Vec::new(),
        private: false,
        source: None,
    };
    let error = invalid_info
        .verify(&target, &VerificationOptions::default(), None)
        .expect_err("piece count mismatch must fail");
    assert!(matches!(error, TrError::InvalidTorrent(_)));
}

#[test]
fn parser_rejects_malformed_data() {
    for malformed in [
        &b""[..],
        &b"le"[..],
        &b"d4:infoe"[..],
        &b"d4:infod6:lengthi1eee"[..],
    ] {
        assert!(Torrent::from_bytes(malformed).is_err());
    }
}

#[test]
fn file_writes_respect_overwrite_policy() {
    let test_dir = TestDir::new("overwrite");
    let target = test_dir.path().join("payload.bin");
    let torrent_path = test_dir.path().join("payload.torrent");
    fs::write(&target, b"abc").expect("write payload");
    let mut torrent = empty_torrent();
    torrent
        .create_torrent(&target, &create_options(4), None)
        .expect("create torrent");

    torrent
        .write_to_file(&torrent_path, false)
        .expect("initial write succeeds");
    let error = torrent
        .write_to_file(&torrent_path, false)
        .expect_err("second write is rejected");
    assert!(matches!(error, TrError::IO(error) if error.kind() == ErrorKind::AlreadyExists));
    torrent
        .write_to_file(&torrent_path, true)
        .expect("forced write succeeds");
}

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
