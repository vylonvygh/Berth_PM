use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ASSET: &str = "berth-linux-test-asset";

const FAKE_CURL: &str = r#"#!/bin/sh
set -eu
url=""
out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            out="$2"
            shift 2
            ;;
        -*)
            shift
            ;;
        *)
            url="$1"
            shift
            ;;
    esac
done
case "$url" in
    */releases/latest) source="$FAKE_RELEASE_JSON" ;;
    */checksums.txt) source="$FAKE_CHECKSUMS" ;;
    */berth-linux-test-asset) source="$FAKE_ASSET" ;;
    *) exit 1 ;;
esac
if [ -n "$out" ]; then
    cat "$source" > "$out"
else
    cat "$source"
fi
"#;

struct InstallerFixture {
    temp: tempfile::TempDir,
    bin: PathBuf,
    asset: PathBuf,
    checksums: PathBuf,
    release: PathBuf,
}

impl InstallerFixture {
    fn new(checksum: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();

        let asset = temp.path().join(ASSET);
        fs::write(&asset, b"binary").unwrap();
        let checksums = temp.path().join("checksums.txt");
        fs::write(&checksums, format!("{}  {}\n", checksum, ASSET)).unwrap();
        let release = temp.path().join("release.json");
        fs::write(&release, "{\"tag_name\":\"v0.1.0-alpha.1\"}\n").unwrap();
        let curl = bin.join("curl");
        fs::write(&curl, FAKE_CURL).unwrap();
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();

        Self {
            temp,
            bin,
            asset,
            checksums,
            release,
        }
    }

    fn installed(&self) -> PathBuf {
        self.temp.path().join("installed/bh")
    }
}

fn run_installer(fixture: &InstallerFixture) -> Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh");
    Command::new("sh")
        .arg(script)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.bin.display(),
                env::var("PATH").unwrap_or_default()
            ),
        )
        .env("BERTH_INSTALL_DIR", fixture.temp.path().join("installed"))
        .env("BERTH_REPO", "acme/berth")
        .env("BERTH_ASSET", ASSET)
        .env("BERTH_GITHUB_API_BASE", "http://example.test")
        .env("BERTH_GITHUB_DOWNLOAD_BASE", "http://example.test")
        .env("FAKE_ASSET", &fixture.asset)
        .env("FAKE_CHECKSUMS", &fixture.checksums)
        .env("FAKE_RELEASE_JSON", &fixture.release)
        .output()
        .unwrap()
}

fn binary_checksum() -> String {
    hex::encode(Sha256::digest(b"binary"))
}

#[test]
fn installer_downloads_verifies_and_installs_executable() {
    let fixture = InstallerFixture::new(&binary_checksum());
    let output = run_installer(&fixture);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(fixture.installed()).unwrap(), b"binary");
    let mode = fs::metadata(fixture.installed())
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
}

#[test]
fn installer_refuses_bad_checksum() {
    let fixture = InstallerFixture::new(&"0".repeat(64));
    let output = run_installer(&fixture);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sha256 mismatch"));
    assert!(!fixture.installed().exists());
}
