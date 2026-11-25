fn main() {
    // env!("KROMA_BUILD_TARGET") for the anonymous heartbeat, which reports the
    // platform this server runs on.
    println!(
        "cargo:rustc-env=KROMA_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    // env!("KROMA_GIT_HASH") for the admin "Version installée" row and for the
    // anonymous heartbeat. The build supplies it where it can: a release
    // compiles inside a container over a bind-mounted checkout, which git
    // refuses to read as a repository nobody in there owns, so asking git at
    // this point answered "unknown" for every published build.
    let commit = from_env("KROMA_GIT_HASH")
        .or_else(|| output("git", &["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=KROMA_GIT_HASH={commit}");

    // `date -u` exists on the Linux/macOS build hosts; anything else falls back
    // to "unknown".
    let date =
        output("date", &["-u", "+%Y-%m-%d %H:%M UTC"]).unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=KROMA_BUILD_DATE={date}");

    let version = from_env("KROMA_VERSION")
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=KROMA_VERSION={version}");
    let build = from_env("KROMA_BUILD").unwrap_or_else(|| "0".to_owned());
    println!("cargo:rustc-env=KROMA_BUILD={build}");

    println!("cargo:rerun-if-env-changed=KROMA_GIT_HASH");
    println!("cargo:rerun-if-env-changed=KROMA_VERSION");
    println!("cargo:rerun-if-env-changed=KROMA_BUILD");
    println!("cargo:rerun-if-changed=../.git/HEAD");

    embed_windows_icon(&version);
}

/// Give the Windows binary its own icon and version strings, so the portable
/// build shows KROMA in Explorer, the taskbar and Alt-Tab rather than a generic
/// executable glyph. The `.ico` is the same one the desktop client ships.
///
/// `embed-resource` finds `rc.exe` (or falls back to a prebuilt parser), runs
/// it and wires the `.res` into the link for every target layout. A machine
/// with no resource toolchain still builds: it just gets an iconless binary.
fn embed_windows_icon(version: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::path::Path::new("resources/kroma.ico").is_file() {
        println!("cargo:warning=resources/kroma.ico missing; the binary will have no icon");
        return;
    }
    // Four 16-bit fields, comma separated, as VERSIONINFO wants them.
    let mut parts: Vec<u16> = version
        .split('.')
        .take(4)
        .map(|p| p.parse::<u16>().unwrap_or(0))
        .collect();
    while parts.len() < 4 {
        parts.push(0);
    }
    let quadruple = format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3]);
    std::fs::write(
        "resources/kroma.rc",
        format!(
            "1 ICON \"kroma.ico\"
             1 VERSIONINFO
             FILEVERSION {q}
             PRODUCTVERSION {q}
             BEGIN
             BLOCK \"StringFileInfo\"
             BEGIN
             BLOCK \"040904B0\"
             BEGIN
             VALUE \"FileDescription\", \"KROMA\"
             VALUE \"FileVersion\", \"{v}\"
             VALUE \"ProductName\", \"KROMA\"
             VALUE \"ProductVersion\", \"{v}\"
             VALUE \"OriginalFilename\", \"kroma-server.exe\"
             END
             END
             BLOCK \"VarFileInfo\"
             BEGIN
             VALUE \"Translation\", 0x409, 1200
             END
             END
",
            q = quadruple,
            v = version,
        ),
    )
    .expect("write resources/kroma.rc");
    // Paths in the .rc are relative to the manifest dir, which is where the
    // resource compiler runs.
    std::env::set_current_dir(
        std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("resources"),
    )
    .expect("chdir to resources/");
    embed_resource::compile("kroma.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile resources/kroma.rc");
    println!("cargo:rerun-if-changed=resources/kroma.ico");
}

fn from_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
