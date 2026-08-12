#![cfg(feature = "cli")]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "torrent_utils_cli_{name}_{}_{}",
            std::process::id(),
            unique
        ));
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn missing_config(&self) -> String {
        self.0.join("missing-config.toml").display().to_string()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn binary_path() -> OsString {
    std::env::var_os("TORRENTUTILSR_TEST_BIN")
        .or_else(|| option_env!("CARGO_BIN_EXE_TorrentUtilsR").map(OsString::from))
        .expect("set TORRENTUTILSR_TEST_BIN or run this test through Cargo")
}

fn run(args: &[String]) -> Output {
    Command::new(binary_path())
        .args(args)
        .output()
        .expect("run TorrentUtilsR")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn version_and_help_are_available() {
    let version = run(&[String::from("-v")]);
    assert!(version.status.success(), "{}", text(&version.stderr));
    let expected_version = format!("TorrentUtilsR {}", env!("CARGO_PKG_VERSION"));
    assert!(text(&version.stdout).starts_with(&expected_version));

    let help = run(&[String::from("-h")]);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let stdout = text(&help.stdout);
    assert!(stdout.contains("A utility for working with torrent files."));
    assert!(stdout.contains("--print-tree"));
    assert!(stdout.contains("--version"));
}

#[test]
fn single_file_create_info_tree_and_verify_lifecycle() {
    let test_dir = TestDir::new("lifecycle");
    let payload = test_dir.path().join("payload.bin");
    let torrent = test_dir.path().join("payload.torrent");
    fs::write(&payload, vec![0x5a; 20_000]).expect("write payload");

    let create = run(&[
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-o"),
        path_string(&torrent),
        String::from("-l"),
        String::from("14"),
        String::from("-q"),
        String::from("-d"),
        String::from("-p"),
        String::from("-s"),
        String::from("compat-suite"),
        String::from("-c"),
        String::from("test comment"),
        String::from("-a"),
        String::from("https://tracker.example/announce"),
    ]);
    assert!(create.status.success(), "{}", text(&create.stderr));
    assert!(torrent.is_file());

    let info = run(&[
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(info.status.success(), "{}", text(&info.stderr));
    let stdout = text(&info.stdout);
    assert!(stdout.contains("Name: payload.bin"));
    assert!(stdout.contains("Tier 0: https://tracker.example/announce"));
    assert!(stdout.contains("Comment: test comment"));
    assert!(stdout.contains("Private: true"));
    assert!(stdout.contains("Source: compat-suite"));
    assert!(stdout.contains("Pieces: 2"));
    assert!(!stdout.contains("Creation date:"));

    let tree = run(&[
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
        String::from("-t"),
    ]);
    assert!(tree.status.success(), "{}", text(&tree.stderr));
    assert!(text(&tree.stdout).contains("[Single file, 20000 (19.53 KiB)]"));

    let verify = run(&[
        path_string(&torrent),
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(verify.status.success(), "{}", text(&verify.stderr));
    assert_eq!(
        text(&verify.stdout),
        concat!(
            "Verification Result:\n",
            "Pieces:        2 total =        2 passed +        0 failed\n",
            "Files:         1 total =        1 passed +        0 failed\n",
            "\n",
            "✓ All files are OK.\n",
        )
    );
    assert!(verify.stderr.is_empty());

    fs::write(&payload, vec![0x33; 20_000]).expect("corrupt payload");
    let verify = run(&[
        path_string(&payload),
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(verify.status.success(), "{}", text(&verify.stderr));
    assert_eq!(
        text(&verify.stdout),
        concat!(
            "Verification Result:\n",
            "Pieces:        2 total =        0 passed +        2 failed\n",
            "Files:         1 total =        0 passed +        1 failed\n",
            "\n",
            "⚠ Some files failed verification:\n",
            "- payload.bin (20000 [19.53 KiB])\n",
        )
    );
    assert!(verify.stderr.is_empty());
}

#[test]
fn directory_tree_and_config_file_are_supported() {
    let test_dir = TestDir::new("directory");
    let payload = test_dir.path().join("payload");
    let nested = payload.join("nested");
    let torrent = test_dir.path().join("directory.torrent");
    let config = test_dir.path().join("config.toml");
    fs::create_dir(&payload).expect("create payload directory");
    fs::create_dir(&nested).expect("create nested directory");
    fs::write(payload.join("a.txt"), b"a").expect("write first file");
    fs::write(nested.join("b.txt"), b"bb").expect("write second file");
    fs::write(
        &config,
        concat!(
            "piece_size = 14\n",
            "private = true\n",
            "source = \"config-source\"\n",
            "tracker_list = [\"https://config.example/announce\"]\n",
            "walk_mode = 1\n",
        ),
    )
    .expect("write config");

    let create = run(&[
        path_string(&payload),
        String::from("-g"),
        path_string(&config),
        String::from("-o"),
        path_string(&torrent),
        String::from("-q"),
        String::from("-d"),
    ]);
    assert!(create.status.success(), "{}", text(&create.stderr));

    let info = run(&[
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(info.status.success(), "{}", text(&info.stderr));
    let stdout = text(&info.stdout);
    assert!(stdout.contains("Private: true"));
    assert!(stdout.contains("Source: config-source"));
    assert!(stdout.contains("Tier 0: https://config.example/announce"));
    assert!(stdout.contains("Files:  2"));

    let tree = run(&[
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
        String::from("-t"),
    ]);
    assert!(tree.status.success(), "{}", text(&tree.stderr));
    let stdout = text(&tree.stdout);
    assert!(stdout.contains("├── a.txt"));
    assert!(stdout.contains("└── nested"));
    assert!(stdout.contains("    └── b.txt"));
}

#[test]
fn overwrite_requires_force() {
    let test_dir = TestDir::new("overwrite");
    let payload = test_dir.path().join("payload.bin");
    let torrent = test_dir.path().join("payload.torrent");
    fs::write(&payload, b"first").expect("write payload");
    let base_args = vec![
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-o"),
        path_string(&torrent),
        String::from("-q"),
        String::from("-d"),
    ];

    let first = run(&base_args);
    assert!(first.status.success(), "{}", text(&first.stderr));
    let original = fs::read(&torrent).expect("read original torrent");

    fs::write(&payload, b"second payload").expect("change payload");
    let refused = run(&base_args);
    assert_eq!(refused.status.code(), Some(1));
    assert!(text(&refused.stderr).contains("File already exists"));
    assert_eq!(
        fs::read(&torrent).expect("read unchanged torrent"),
        original
    );

    let mut forced_args = base_args;
    forced_args.push(String::from("-f"));
    let forced = run(&forced_args);
    assert!(forced.status.success(), "{}", text(&forced.stderr));
    assert_ne!(fs::read(&torrent).expect("read replaced torrent"), original);
}

#[test]
fn invalid_arguments_return_actionable_errors() {
    let no_input = run(&[]);
    assert_eq!(no_input.status.code(), Some(1));
    assert!(text(&no_input.stderr).contains("Please provide one target"));

    let test_dir = TestDir::new("invalid_args");
    let payload = test_dir.path().join("payload.bin");
    fs::write(&payload, b"data").expect("write payload");
    let invalid_piece = run(&[
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-l"),
        String::from("13"),
        String::from("-q"),
    ]);
    assert_eq!(invalid_piece.status.code(), Some(1));
    assert!(text(&invalid_piece.stderr).contains("Piece size must be between 14 and 27"));

    let two_plain_inputs = run(&[
        path_string(&payload),
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert_eq!(two_plain_inputs.status.code(), Some(1));
    assert!(text(&two_plain_inputs.stderr).contains("provide a .torrent file"));
}

#[test]
fn empty_file_torrent_can_be_read_and_verified() {
    let test_dir = TestDir::new("empty_file");
    let payload = test_dir.path().join("empty.bin");
    let torrent = test_dir.path().join("empty.torrent");
    fs::write(&payload, []).expect("write empty payload");

    let create = run(&[
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-o"),
        path_string(&torrent),
        String::from("-q"),
        String::from("-d"),
    ]);
    assert!(create.status.success(), "{}", text(&create.stderr));

    let info = run(&[
        path_string(&torrent),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(info.status.success(), "{}", text(&info.stderr));
    assert!(text(&info.stdout).contains("Pieces: 0"));

    let verify = run(&[
        path_string(&torrent),
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert!(verify.status.success(), "{}", text(&verify.stderr));
    assert_eq!(
        text(&verify.stdout),
        concat!(
            "Verification Result:\n",
            "Pieces:        0 total =        0 passed +        0 failed\n",
            "Files:         1 total =        1 passed +        0 failed\n",
            "\n",
            "✓ All files are OK.\n",
        )
    );
    assert!(verify.stderr.is_empty());
}

#[test]
fn inconsistent_piece_count_is_reported_without_panicking() {
    let test_dir = TestDir::new("piece_count");
    let payload = test_dir.path().join("payload.bin");
    let torrent = test_dir.path().join("malformed.torrent");
    fs::write(&payload, b"abc").expect("write payload");
    fs::write(
        &torrent,
        b"d4:infod6:lengthi3e4:name11:payload.bin12:piece lengthi2e6:pieces0:ee",
    )
    .expect("write malformed torrent");

    let verify = run(&[
        path_string(&torrent),
        path_string(&payload),
        String::from("-g"),
        test_dir.missing_config(),
        String::from("-q"),
    ]);
    assert_eq!(verify.status.code(), Some(1), "{}", text(&verify.stderr));
    let stderr = text(&verify.stderr);
    assert!(stderr.contains("piece count mismatch"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}
