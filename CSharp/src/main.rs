use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

struct Engine {
    program: String,
    args: Vec<String>,
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let engine = match resolve_engine() {
        Ok(engine) => engine,
        Err(err) => {
            eprintln!("forge-lsp-csharp: {err}");
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

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!(
                "forge-lsp-csharp: failed to start {}: {err}",
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
    if let Some(path) = find_roslyn_tool() {
        return Ok(roslyn_engine(path));
    }

    if which("dotnet").is_some() && install_roslyn_tool() && find_roslyn_tool().is_some() {
        if let Some(path) = find_roslyn_tool() {
            return Ok(roslyn_engine(path));
        }
    }

    if let Some((exe, razor_dll, razor_design_time, csharp_design_time)) = find_vscode_roslyn() {
        let log_dir = env::temp_dir()
            .join("roslyn-forge")
            .to_string_lossy()
            .into_owned();

        let mut args = vec![
            "--stdio".to_string(),
            "--extension".to_string(),
            razor_dll,
            "--extensionLogDirectory".to_string(),
            log_dir,
            "--telemetryLevel".to_string(),
            "off".to_string(),
        ];

        if let Some(path) = razor_design_time {
            args.push("--razorDesignTimePath".to_string());
            args.push(path);
        }
        if let Some(path) = csharp_design_time {
            args.push("--csharpDesignTimePath".to_string());
            args.push(path);
        }

        return Ok(Engine {
            program: exe,
            args,
        });
    }

    if let Some(omnisharp) = which("omnisharp") {
        return Ok(Engine {
            program: omnisharp,
            args: vec!["-stdio".to_string()],
        });
    }

    if let Some(csharp_ls) = which("csharp-ls") {
        return Ok(Engine {
            program: csharp_ls,
            args: vec![],
        });
    }

    Err(
        "no C# language server available. Tried:\n\
         - roslyn-language-server (dotnet tool; auto-install failed)\n\
         - VS Code C# extension (ms-dotnettools.csharp)\n\
         - OmniSharp on PATH\n\
         - csharp-ls on PATH\n\n\
         Install manually with: dotnet tool install -g roslyn-language-server"
            .to_string(),
    )
}

fn roslyn_engine(program: String) -> Engine {
    let mut args = vec![
        "--stdio".to_string(),
        "--autoLoadProjects".to_string(),
        "--clientProcessId".to_string(),
        std::process::id().to_string(),
    ];

    if let Some(dll) = find_razor_extension_dll() {
        args.push("--extension".to_string());
        args.push(dll);
    }

    Engine { program, args }
}

fn find_roslyn_tool() -> Option<String> {
    if let Some(path) = which("roslyn-language-server") {
        return Some(path);
    }

    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .ok()?;
    let candidate = PathBuf::from(&home)
        .join(".dotnet")
        .join("tools")
        .join(if cfg!(windows) {
            "roslyn-language-server.exe"
        } else {
            "roslyn-language-server"
        });
    candidate.is_file().then(|| candidate.to_string_lossy().into_owned())
}

fn install_roslyn_tool() -> bool {
    eprintln!("forge-lsp-csharp: installing roslyn-language-server via dotnet tool...");

    let mut cmd = Command::new("dotnet");
    cmd.args(["tool", "install", "-g", "roslyn-language-server"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    matches!(cmd.status(), Ok(status) if status.success())
}

fn find_vscode_roslyn() -> Option<(String, String, Option<String>, Option<String>)> {
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .ok()?;
    let extensions_dir = PathBuf::from(&home).join(".vscode").join("extensions");

    let ext_dir = fs::read_dir(&extensions_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("ms-dotnettools.csharp-"))
                .unwrap_or(false)
        })?;

    let server_binary = if cfg!(windows) {
        "Microsoft.CodeAnalysis.LanguageServer.exe"
    } else {
        "Microsoft.CodeAnalysis.LanguageServer"
    };

    let roslyn_exe = ext_dir.join(".roslyn").join(server_binary);
    let razor_dll = ext_dir
        .join(".razorExtension")
        .join("Microsoft.VisualStudioCode.RazorExtension.dll");

    if !roslyn_exe.exists() || !razor_dll.exists() {
        return None;
    }

    let design_time = |file: &str| {
        let path = ext_dir.join(".razorExtension").join("Targets").join(file);
        path.exists().then(|| path.to_string_lossy().into_owned())
    };

    Some((
        roslyn_exe.to_string_lossy().into_owned(),
        razor_dll.to_string_lossy().into_owned(),
        design_time("Microsoft.NET.Sdk.Razor.DesignTime.targets"),
        design_time("Microsoft.CSharpExtension.DesignTime.targets"),
    ))
}

fn find_razor_extension_dll() -> Option<String> {
    let home = env::var("USERPROFILE").or_else(|_| env::var("HOME")).ok()?;
    let extensions_dirs = [
        Path::new(&home).join(".vscode/extensions"),
        Path::new(&home).join(".vscode-insiders/extensions"),
    ];

    for dir in &extensions_dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("ms-dotnettools.csharp-"))
            })
            .collect();
        candidates.sort();
        for ext_dir in candidates.into_iter().rev() {
            let dll = ext_dir
                .join(".razorExtension")
                .join("Microsoft.VisualStudioCode.RazorExtension.dll");
            if dll.exists() {
                return Some(dll.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    None
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
