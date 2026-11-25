use flate2::{write::GzEncoder, Compression};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use tar::{Builder, Header};

struct StubRelease {
    tag: String,
    asset: Vec<u8>,
    digest: String,
    checksum: String,
}

impl StubRelease {
    fn new(tag: &str, binary: &[u8]) -> Self {
        let asset = tar_gz("pkg/tool", binary);
        let checksum = hex::encode(Sha256::digest(&asset));
        Self {
            tag: tag.to_string(),
            asset,
            digest: format!("sha256:{}", checksum),
            checksum,
        }
    }

    fn with_bad_checksum(mut self) -> Self {
        self.digest = format!("sha256:{}", "0".repeat(64));
        self
    }
}

struct MockState {
    releases: Vec<StubRelease>,
    latest: usize,
}

struct MockGithub {
    addr: String,
    state: Arc<Mutex<MockState>>,
}

impl MockGithub {
    fn new(releases: Vec<StubRelease>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(MockState {
            releases,
            latest: 0,
        }));
        let server_state = state.clone();
        let server_addr = addr.clone();
        thread::spawn(move || serve(listener, server_state, server_addr));
        Self { addr, state }
    }

    fn set_latest(&self, index: usize) {
        self.state.lock().unwrap().latest = index;
    }
}

fn serve(listener: TcpListener, state: Arc<Mutex<MockState>>, addr: String) {
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(_) => break,
        };
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        let path = String::from_utf8_lossy(&request)
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();

        if path.contains("/releases/latest") {
            let (tag, digest) = {
                let guard = state.lock().unwrap();
                let release = &guard.releases[guard.latest];
                (release.tag.clone(), release.digest.clone())
            };
            let body = serde_json::json!({
                "tag_name": tag,
                "assets": [
                    {
                        "name": "tool-linux-x86_64.tar.gz",
                        "browser_download_url": format!("http://{}/asset", addr),
                        "digest": digest
                    },
                    {
                        "name": "checksums.txt",
                        "browser_download_url": format!("http://{}/checksums", addr),
                        "digest": null
                    }
                ]
            })
            .to_string();
            respond(&mut stream, "200 OK", "application/json", body.as_bytes());
            continue;
        }
        if path.contains("/checksums") {
            let checksum = {
                let guard = state.lock().unwrap();
                guard.releases[guard.latest].checksum.clone()
            };
            let body = format!("{}  tool-linux-x86_64.tar.gz\n", checksum);
            respond(&mut stream, "200 OK", "text/plain", body.as_bytes());
            continue;
        }
        if path.contains("/asset") {
            let asset = {
                let guard = state.lock().unwrap();
                guard.releases[guard.latest].asset.clone()
            };
            respond(&mut stream, "200 OK", "application/octet-stream", &asset);
            continue;
        }
        respond(&mut stream, "404 Not Found", "text/plain", b"not found");
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

fn tar_gz(member: &str, contents: &[u8]) -> Vec<u8> {
    let mut archive = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.as_mut_bytes()[..member.len()].copy_from_slice(member.as_bytes());
    header.set_cksum();
    archive.append(&header, contents).unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

fn berth(home: &Path, mock: &MockGithub, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_berth"))
        .args(args)
        .env("HOME", home)
        .env("BERTH_GITHUB_API_BASE", format!("http://{}", mock.addr))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn add_list_info_sync_status_and_remove_use_managed_symlinks() {
    let home = tempfile::tempdir().unwrap();
    let binary = b"#!/bin/sh\necho one\n";
    let mock = MockGithub::new(vec![StubRelease::new("v1.0.0", binary)]);

    let added = berth(
        home.path(),
        &mock,
        &["add", "tool", "--repo", "acme/tool", "--bin-name", "tool"],
    );
    assert!(
        added.status.success(),
        "{}{}",
        stdout(&added),
        stderr(&added)
    );
    assert!(stdout(&added).contains("tool 1.0.0 installed"));

    let link = home.path().join(".local/bin/tool");
    let target = fs::read_link(&link).unwrap();
    assert!(target.to_string_lossy().contains("tool/1.0.0/tool"));
    assert_eq!(fs::read(&target).unwrap(), binary);

    let listed = berth(home.path(), &mock, &["list", "--json", "--no-color"]);
    let rows: Value = serde_json::from_str(&stdout(&listed)).unwrap();
    assert_eq!(rows[0]["name"], "tool");
    assert_eq!(rows[0]["version"], "1.0.0");

    let info = berth(home.path(), &mock, &["info", "tool"]);
    assert!(stdout(&info).contains("sha256:"));

    fs::remove_file(&link).unwrap();
    let synced = berth(home.path(), &mock, &["sync"]);
    assert!(synced.status.success());
    assert!(link.exists());

    let status = berth(home.path(), &mock, &["status"]);
    assert!(status.status.success(), "{}", stderr(&status));

    let removed = berth(home.path(), &mock, &["remove", "tool"]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    assert!(!link.exists());
    assert!(target.is_file(), "cached binary should survive removal");
}

#[test]
fn update_keeps_old_version_and_recovers_from_bad_checksum() {
    let home = tempfile::tempdir().unwrap();
    let mock = MockGithub::new(vec![
        StubRelease::new("v1.0.0", b"one"),
        StubRelease::new("v2.0.0", b"two"),
        StubRelease::new("v3.0.0", b"three").with_bad_checksum(),
    ]);

    let added = berth(
        home.path(),
        &mock,
        &[
            "add",
            "tool",
            "--repo",
            "acme/tool",
            "--bin-name",
            "tool",
            "--quiet",
        ],
    );
    assert!(added.status.success(), "{}", stderr(&added));

    mock.set_latest(1);
    let updated = berth(home.path(), &mock, &["update", "tool", "--quiet"]);
    assert!(updated.status.success(), "{}", stderr(&updated));
    let link = home.path().join(".local/bin/tool");
    assert!(fs::read_link(&link)
        .unwrap()
        .to_string_lossy()
        .contains("tool/2.0.0/tool"));

    mock.set_latest(2);
    let failed = berth(home.path(), &mock, &["update", "tool", "--quiet"]);
    assert!(!failed.status.success());
    assert!(
        stderr(&failed).contains("sha256 mismatch"),
        "{}",
        stderr(&failed)
    );
    assert!(fs::read_link(&link)
        .unwrap()
        .to_string_lossy()
        .contains("tool/2.0.0/tool"));
    let status = berth(home.path(), &mock, &["status", "--quiet"]);
    assert!(status.status.success(), "{}", stderr(&status));

    let rolled_back = berth(home.path(), &mock, &["rollback", "tool", "--quiet"]);
    assert!(rolled_back.status.success(), "{}", stderr(&rolled_back));
    assert!(fs::read_link(&link)
        .unwrap()
        .to_string_lossy()
        .contains("tool/1.0.0/tool"));
}

#[test]
fn refuses_unsafe_archive_and_keeps_json_output_clean() {
    let home = tempfile::tempdir().unwrap();
    let unsafe_archive = tar_gz("../../evil", b"bad");
    let checksum = hex::encode(Sha256::digest(&unsafe_archive));
    let mut release = StubRelease::new("v1.0.0", b"ignored");
    release.asset = unsafe_archive;
    release.checksum = checksum.clone();
    release.digest = format!("sha256:{}", checksum);
    let mock = MockGithub::new(vec![release]);

    let added = berth(
        home.path(),
        &mock,
        &[
            "add",
            "tool",
            "--repo",
            "acme/tool",
            "--bin-name",
            "tool",
            "--quiet",
        ],
    );
    assert!(!added.status.success());
    assert!(stderr(&added).contains("no safe"), "{}", stderr(&added));
    assert!(!home.path().join(".local/bin/tool").exists());

    let listing = berth(
        home.path(),
        &mock,
        &["list", "--json", "--quiet", "--no-color"],
    );
    assert!(!stdout(&listing).contains('\u{1b}'));
    assert_eq!(stdout(&listing).trim_end(), "[]");
}
