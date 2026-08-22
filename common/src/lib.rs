//! Shared plumbing for Forge stdio-proxy language servers.
//!
//! Each language package (`Rust/`, `Cpp/`, ...) is a thin binary that describes
//! its upstream engine via [`EngineSpec`] and calls [`run`], which:
//!
//! 1. probes `PATH` for an already-installed engine ([`PathCandidate`]s first —
//!    a locally installed, toolchain-matched engine beats a downloaded one),
//! 2. otherwise downloads a pinned GitHub release asset into a per-user cache
//!    (`%LOCALAPPDATA%\forge\lsp-engines\<lang>\<tag>\`, Unix:
//!    `~/.local/share/forge/lsp-engines/...`) and extracts zip, tar.gz, gz, or
//!    raw-binary payloads automatically,
//! 3. spawns the engine and pipes LSP bytes between its stdio and ours.
//!
//! stdout is reserved for LSP frames; every log line and error goes to stderr,
//! and failures exit with code 1.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use flate2::read::GzDecoder;
use serde_json::Value;

// ── Platform ─────────────────────────────────────────────────────────────────

/// The Forge platform id for the current host (`windows-x86_64`, `macos-aarch64`, ...).
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("macos", "aarch64") => "macos-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "unknown",
    }
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

/// The engine executable name for this host, e.g. `clangd.exe` / `clangd`.
pub fn exe_name(binary_base: &str) -> String {
    format!("{binary_base}{}", exe_suffix())
}

// ── Spec ─────────────────────────────────────────────────────────────────────

/// A `PATH` probe tried before any download. Lets a toolchain-matched local
/// install win (e.g. rustup's rust-analyzer).
#[derive(Clone, Copy)]
pub struct PathCandidate {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// Everything the shared runner needs to know about one upstream engine.
pub struct EngineSpec {
    /// Cache subdir + log prefix (`forge-lsp-<lang>`).
    pub lang: &'static str,
    /// `owner/repo` hosting the releases.
    pub repo: &'static str,
    /// Pinned release tag (recommended), or empty to track the latest release.
    pub tag: &'static str,
    /// Maps a Forge platform key to the release asset serving it; `None` means
    /// upstream ships nothing for that platform and the user must install manually.
    pub platform_asset: fn(&str) -> Option<&'static str>,
    /// Executable base name to locate inside the archive (`exe_name` adds `.exe`).
    pub binary_base: &'static str,
    /// Engines found under these names on `PATH` are spawned directly.
    pub path_candidates: &'static [PathCandidate],
    /// Extra args passed to the downloaded engine.
    pub engine_args: &'static [&'static str],
    /// Run the engine with cwd = extraction root (engines needing sibling resources).
    pub cwd_engine_root: bool,
}

// ── Runner ───────────────────────────────────────────────────────────────────

/// Resolves, launches, and pumps one engine per `spec`. Blocks until the LSP
/// session ends; returns the process exit code.
pub fn run(spec: &EngineSpec) -> i32 {
    for candidate in spec.path_candidates {
        if let Some(path) = which(candidate.program) {
            return spawn_and_pump(&path, candidate.args, None);
        }
    }

    let engine = match ensure_engine(spec) {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("forge-lsp-{}: {err}", spec.lang);
            return 1;
        }
    };

    spawn_and_pump(&engine.exe, spec.engine_args, engine.cwd.as_deref())
}

struct ResolvedEngine {
    exe: PathBuf,
    cwd: Option<PathBuf>,
}

fn ensure_engine(spec: &EngineSpec) -> Result<ResolvedEngine, String> {
    let platform = platform_key();
    let asset_name = (spec.platform_asset)(platform).ok_or_else(|| {
        format!(
            "{repo} publishes no prebuilt engine for platform {platform}; install {base} manually and put it on PATH",
            repo = spec.repo,
            base = spec.binary_base
        )
    })?;

    let expected = exe_name(spec.binary_base);
    let lang_root = cache_root().join(spec.lang);
    // Flatten tags containing '/' (e.g. "typescript/v7.0.2") so the cache
    // stays one level deep — prune_old_versions relies on that layout.
    let dir = lang_root.join(if spec.tag.is_empty() {
        "latest".to_string()
    } else {
        spec.tag.replace('/', "_")
    });

    if dir.join(".forge-ok").exists() {
        if let Some(exe) = find_binary(&dir, &expected) {
            return Ok(wrap(spec, exe, &dir));
        }
        eprintln!("forge-lsp-{}: cached copy is incomplete, re-downloading...", spec.lang);
    }

    eprintln!(
        "forge-lsp-{lang}: downloading {repo} [{asset}]...",
        lang = spec.lang,
        repo = spec.repo,
        asset = asset_name
    );
    let bytes = download_release_asset(spec, asset_name)?;

    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|err| format!("clearing {}: {err}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|err| format!("creating {}: {err}", dir.display()))?;

    extract_auto(&bytes, &dir, &expected)?;
    File::create(dir.join(".forge-ok"))
        .map_err(|err| format!("marking {}: {err}", dir.display()))?;
    prune_old_versions(&lang_root, dir.file_name());

    let exe = find_binary(&dir, &expected)
        .ok_or_else(|| format!("{expected} not found after extracting {asset_name}"))?;
    Ok(wrap(spec, exe, &dir))
}

fn wrap(spec: &EngineSpec, exe: PathBuf, dir: &Path) -> ResolvedEngine {
    ResolvedEngine {
        exe,
        cwd: spec.cwd_engine_root.then(|| dir.to_path_buf()),
    }
}

fn download_release_asset(spec: &EngineSpec, asset_name: &str) -> Result<Vec<u8>, String> {
    let agent = format!("forge-lsp-{}", spec.lang);
    let endpoint = if spec.tag.is_empty() {
        format!("https://api.github.com/repos/{}/releases/latest", spec.repo)
    } else {
        format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            spec.repo, spec.tag
        )
    };

    let body = ureq::get(&endpoint)
        .set("User-Agent", &agent)
        .call()
        .map_err(|err| format!("querying releases for {}: {err}", spec.repo))?
        .into_string()
        .map_err(|err| format!("reading releases response for {}: {err}", spec.repo))?;
    let release: Value = serde_json::from_str(&body)
        .map_err(|err| format!("parsing releases response for {}: {err}", spec.repo))?;

    let url = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                if asset["name"].as_str() == Some(asset_name) {
                    asset["browser_download_url"].as_str().map(str::to_string)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| format!("no asset '{asset_name}' in {} release", spec.repo))?;

    let mut reader = ureq::get(&url)
        .set("User-Agent", &agent)
        .call()
        .map_err(|err| format!("downloading {url}: {err}"))?
        .into_reader();

    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|err| format!("reading {url}: {err}"))?;
    Ok(bytes)
}

// ── Extraction ───────────────────────────────────────────────────────────────

const XZ_MAGIC: &[u8] = &[0xFD, b'7', b'Z', b'X', b'Z', 0x00];

/// Sniffs `bytes` and extracts accordingly: zip, tar.gz, gzip'd single file,
/// tar.xz (via system `tar`, Unix only), or a raw single binary written as
/// `single_file_name`.
pub fn extract_auto(bytes: &[u8], dest: &Path, single_file_name: &str) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|err| format!("creating {}: {err}", dest.display()))?;

    if bytes.starts_with(b"PK") {
        extract_zip(bytes, dest)
    } else if bytes.starts_with(&[0x1F, 0x8B]) {
        let mut payload = Vec::new();
        GzDecoder::new(bytes)
            .read_to_end(&mut payload)
            .map_err(|err| format!("gunzip: {err}"))?;
        if is_tar(&payload) {
            tar::Archive::new(&payload[..])
                .unpack(dest)
                .map_err(|err| format!("unpacking tar.gz: {err}"))
        } else {
            write_single_file(dest, single_file_name, &payload)
        }
    } else if bytes.starts_with(XZ_MAGIC) {
        extract_tar_xz(bytes, dest)
    } else {
        write_single_file(dest, single_file_name, bytes)
    }
}

fn is_tar(payload: &[u8]) -> bool {
    payload.len() >= 262 && &payload[257..262] == b"ustar"
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(io::Cursor::new(bytes.to_vec())).map_err(|err| format!("zip: {err}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("corrupt archive entry: {err}"))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| format!("creating {}: {err}", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("creating {}: {err}", parent.display()))?;
        }
        let mut out_file = File::create(&out_path)
            .map_err(|err| format!("creating {}: {err}", out_path.display()))?;
        io::copy(&mut entry, &mut out_file)
            .map_err(|err| format!("extracting {}: {err}", out_path.display()))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            if mode & 0o111 != 0 {
                let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(0o755));
            }
        }
    }

    Ok(())
}

fn extract_tar_xz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    if cfg!(windows) {
        return Err(
            "tar.xz engines are not supported on Windows; no zip asset exists for this platform"
                .to_string(),
        );
    }

    let temp_archive = dest.with_extension("tar.xz.tmp");
    fs::write(&temp_archive, bytes)
        .map_err(|err| format!("writing {}: {err}", temp_archive.display()))?;

    let result = Command::new("tar")
        .args(["-xJf"])
        .arg(&temp_archive)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|err| format!("running system tar: {err}"))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("system tar failed with {}", status))
            }
        });

    let _ = fs::remove_file(&temp_archive);
    result
}

fn write_single_file(dest: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let out_path = dest.join(name);
    fs::create_dir_all(dest.parent().unwrap_or(dest))
        .map_err(|err| format!("creating {}: {err}", dest.display()))?;
    fs::write(&out_path, bytes).map_err(|err| format!("writing {}: {err}", out_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Recursively finds a file named `expected` (case-insensitive) under `dir`,
/// preferring the shallowest match so versioned/nested wrappers lose.
fn find_binary(dir: &Path, expected: &str) -> Option<PathBuf> {
    let expected = expected.to_ascii_lowercase();
    let mut matches = Vec::new();
    collect_matches(dir, &expected, &mut matches);
    matches.into_iter().min_by_key(|path| path.components().count())
}

fn collect_matches(dir: &Path, expected: &str, matches: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_matches(&path, expected, matches);
        } else if entry.file_name().to_string_lossy().to_ascii_lowercase() == expected {
            matches.push(path);
        }
    }
}

// ── Cache layout ─────────────────────────────────────────────────────────────

fn cache_root() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
            });
        base.join("forge").join("lsp-engines")
    }

    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join(".local").join("share").join("forge").join("lsp-engines")
    }
}

fn prune_old_versions(lang_root: &Path, keep: Option<&std::ffi::OsStr>) {
    let Some(keep) = keep else { return };
    let Ok(entries) = fs::read_dir(lang_root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() && entry.file_name() != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

// ── Process helpers ──────────────────────────────────────────────────────────

/// Applies Windows-only spawn hardening (hide console window) to any command.
pub fn configure_no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
}

/// Case-insensitive-with-PATHEXT `PATH` lookup (`.exe`/`.cmd`/`.bat` on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };

    for dir in std::env::split_paths(&path_var) {
        for extension in extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Spawns `program`, wires our stdin/stdout through to it (stderr inherited),
/// and blocks until it exits. Returns its exit code, or 1 if it could not start.
pub fn spawn_and_pump(program: &Path, args: &[&str], cwd: Option<&Path>) -> i32 {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    configure_no_window(&mut cmd);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("failed to start {}: {err}", program.display());
            return 1;
        }
    };

    let child_stdin = child.stdin.take();
    let mut child_stdout = child.stdout.take();

    let pump = std::thread::spawn(move || {
        if let Some(mut pipe) = child_stdin {
            let _ = io::copy(&mut io::stdin().lock(), &mut pipe);
        }
    });

    let mut stdout = io::stdout().lock();
    if let Some(pipe) = child_stdout.as_mut() {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Flush every chunk: stdout is line-buffered when piped,
                    // and LSP frames rarely end in '\n'.
                    let ok = stdout.write_all(&buf[..n]).is_ok() && stdout.flush().is_ok();
                    if !ok {
                        break;
                    }
                }
            }
        }
    }

    pump.join().ok();
    child.wait().ok().and_then(|status| status.code()).unwrap_or(1)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "forge-proxy-test-{}-{}",
                label,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn extracts_raw_binary_as_single_file() {
        let temp = TempDir::new("raw");
        extract_auto(b"MZ-fake-engine-bytes", temp.path(), &exe_name("tool")).unwrap();

        assert!(temp.path().join(exe_name("tool")).is_file());
        let found = find_binary(temp.path(), &exe_name("tool")).unwrap();
        assert_eq!(found, temp.path().join(exe_name("tool")));
    }

    #[test]
    fn extracts_gzipped_single_file() {
        let payload = b"#!/usr/bin/env fake-engine";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(payload).unwrap();
        let gz = encoder.finish().unwrap();

        let temp = TempDir::new("gz");
        extract_auto(&gz, temp.path(), &exe_name("tool")).unwrap();

        assert_eq!(
            fs::read(temp.path().join(exe_name("tool"))).unwrap(),
            payload
        );
    }

    #[test]
    fn extracts_tar_gz_preserving_paths() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));

        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "pkg/bin/tool", &b"bin!"[..])
            .unwrap();
        let mut header_readme = tar::Header::new_gnu();
        header_readme.set_size(2);
        header_readme.set_cksum();
        builder
            .append_data(&mut header_readme, "pkg/README", &b"hi"[..])
            .unwrap();

        let tarball = builder.into_inner().unwrap().finish().unwrap();

        let temp = TempDir::new("targz");
        extract_auto(&tarball, temp.path(), "tool").unwrap();

        assert_eq!(fs::read(temp.path().join("pkg/bin/tool")).unwrap(), b"bin!");
        assert_eq!(
            find_binary(temp.path(), "tool").unwrap(),
            temp.path().join("pkg/bin/tool")
        );
    }

    #[test]
    fn extracts_zip_preserving_paths() {
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        writer
            .start_file("pkg/bin/tool.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"win-engine").unwrap();
        writer
            .start_file("pkg/data/res.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"data").unwrap();
        let zip_bytes = writer.finish().unwrap().into_inner();

        let temp = TempDir::new("zip");
        extract_auto(&zip_bytes, temp.path(), "tool.exe").unwrap();

        assert_eq!(
            fs::read(temp.path().join("pkg/bin/tool.exe")).unwrap(),
            b"win-engine"
        );
        assert_eq!(
            find_binary(temp.path(), "tool.exe").unwrap(),
            temp.path().join("pkg/bin/tool.exe")
        );
    }

    #[test]
    fn finds_shallowest_binary_match() {
        let temp = TempDir::new("shallow");
        let deep = temp.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("tool"), b"deep").unwrap();
        fs::write(temp.path().join("tool"), b"shallow").unwrap();

        assert_eq!(find_binary(temp.path(), "tool").unwrap(), temp.path().join("tool"));
    }
}
