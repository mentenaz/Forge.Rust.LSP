use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use serde_json::Value;
use zip::ZipArchive;

const REPO: &str = "PowerShell/PowerShellEditorServices";
const ASSET_NAME: &str = "PowerShellEditorServices.zip";
const DIR_PREFIX: &str = "PowerShellEditorServices-";

struct Engine {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let engine = match resolve_engine() {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("forge-lsp-powershell: {err}");
            return 1;
        }
    };

    let mut cmd = Command::new(&engine.program);
    cmd.args(&engine.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    if let Some(cwd) = &engine.cwd {
        cmd.current_dir(cwd);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "forge-lsp-powershell: failed to start {}: {err}",
                engine.program
            );
            return 1;
        }
    };

    let child_stdin = child.stdin.take();
    let mut child_stdout = child.stdout.take();

    let pump = thread::spawn(move || {
        if let Some(mut pipe) = child_stdin {
            let mut stdin = io::stdin().lock();
            let _ = io::copy(&mut stdin, &mut pipe);
        }
    });

    let mut stdout = io::stdout().lock();
    if let Some(pipe) = child_stdout.as_mut() {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stdout.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = stdout.flush();
                }
            }
        }
    }

    let _ = child.kill();
    let status = child.wait();
    let _ = pump.join();

    status.ok().and_then(|s| s.code()).unwrap_or(1)
}

fn resolve_engine() -> Result<Engine, String> {
    let shell = which("pwsh")
        .or_else(|| which("powershell"))
        .ok_or_else(|| {
            "pwsh or powershell executable not found in PATH. Please install PowerShell."
                .to_string()
        })?;

    let script = ensure_editor_services()?;
    let script_abs = script.to_string_lossy().replace('\\', "/");

    let version_dir = match script.parent() {
        Some(dir) if dir.file_name().and_then(|n| n.to_str()) == Some("PowerShellEditorServices") => {
            dir.parent().unwrap_or(dir).to_path_buf()
        }
        Some(dir) => dir.to_path_buf(),
        None => return Err("failed to resolve PowerShellEditorServices directory".to_string()),
    };
    let bundled_modules_path = version_dir.to_string_lossy().replace('\\', "/");

    let args = [
        "-NoLogo",
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        &script_abs,
        "-Stdio",
        "-HostName",
        "Forge",
        "-HostProfileId",
        "Forge",
        "-HostVersion",
        "1.0.0",
        "-BundledModulesPath",
        &bundled_modules_path,
        "-LogLevel",
        "Information",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();

    Ok(Engine {
        program: shell,
        args,
        cwd: Some(version_dir),
    })
}

fn ensure_editor_services() -> Result<PathBuf, String> {
    let root = cache_root();

    if let Ok(entries) = fs::read_dir(&root) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with(DIR_PREFIX))
                    .unwrap_or(false)
            })
            .collect();
        dirs.sort();
        for dir in dirs.into_iter().rev() {
            if let Some(script) = find_start_script(&dir) {
                return Ok(script);
            }
        }
    }

    eprintln!("forge-lsp-powershell: downloading PowerShellEditorServices...");

    let (tag, url) = latest_release()?;
    let version_dir = root.join(format!("{DIR_PREFIX}{tag}"));
    fs::create_dir_all(&version_dir)
        .map_err(|err| format!("failed to create {}: {err}", version_dir.display()))?;

    let zip_path = root.join(ASSET_NAME);
    download(&url, &zip_path)?;
    extract(&zip_path, &version_dir)?;
    let _ = fs::remove_file(&zip_path);

    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name) = name.to_str() {
                if name.starts_with(DIR_PREFIX) && name != format!("{DIR_PREFIX}{tag}") {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    find_start_script(&version_dir)
        .ok_or_else(|| "Start-EditorServices.ps1 not found in downloaded archive".to_string())
}

fn find_start_script(version_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        version_dir
            .join("PowerShellEditorServices")
            .join("Start-EditorServices.ps1"),
        version_dir.join("Start-EditorServices.ps1"),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
}

fn latest_release() -> Result<(String, String), String> {
    let body = ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("User-Agent", "forge-lsp-powershell")
        .call()
        .map_err(|err| format!("failed to query GitHub releases: {err}"))?
        .into_string()
        .map_err(|err| format!("failed to read GitHub releases response: {err}"))?;

    let release: Value = serde_json::from_str(&body)
        .map_err(|err| format!("failed to parse GitHub releases response: {err}"))?;

    let tag = release["tag_name"]
        .as_str()
        .map(|tag| tag.trim_start_matches('v').to_string())
        .ok_or_else(|| "GitHub release response missing tag_name".to_string())?;

    let url = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                if asset["name"].as_str() == Some(ASSET_NAME) {
                    asset["browser_download_url"]
                        .as_str()
                        .map(|url| url.to_string())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| format!("no asset found matching '{ASSET_NAME}'"))?;

    Ok((tag, url))
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    let response = ureq::get(url)
        .set("User-Agent", "forge-lsp-powershell")
        .call()
        .map_err(|err| format!("failed to download {url}: {err}"))?;

    let mut reader = response.into_reader();
    let mut file =
        File::create(dest).map_err(|err| format!("failed to create {}: {err}", dest.display()))?;
    io::copy(&mut reader, &mut file)
        .map_err(|err| format!("failed to download {url}: {err}"))?;
    Ok(())
}

fn extract(archive_path: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|err| format!("failed to open {}: {err}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|err| format!("failed to read {}: {err}", archive_path.display()))?;

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
                .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
            }
            let mut out_file = File::create(&out_path)
                .map_err(|err| format!("failed to create {}: {err}", out_path.display()))?;
            io::copy(&mut entry, &mut out_file)
                .map_err(|err| format!("failed to extract {}: {err}", out_path.display()))?;
        }
    }

    Ok(())
}

fn cache_root() -> PathBuf {
    #[cfg(windows)]
    {
        let base = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_default()
            });
        base.join("forge").join("lsp-engines").join("powershell")
    }

    #[cfg(not(windows))]
    {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join(".local")
            .join("share")
            .join("forge")
            .join("lsp-engines")
            .join("powershell")
    }
}

fn which(name: &str) -> Option<String> {
    let path_var = env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };

    for dir in env::split_paths(&path_var) {
        for extension in extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }

    None
}
