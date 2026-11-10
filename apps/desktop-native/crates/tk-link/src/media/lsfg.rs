//! LSFG (Lossless Scaling Frame Generation) availability on the PC (experimental).
//!
//! Ported from Enigmata-Temple `crates/temple-core/src/media/lsfg.rs` (v0.7.0, commit 34ff76c; AGPL-3.0, same
//! author). LSFG does not run inside FFmpeg: the `temple-lsfg` helper (C++/Vulkan, built from Enigmata-Temple
//! `native/temple-lsfg`) sits between two FFmpeg processes ([`super::args::build_lsfg`]). It needs two files that
//! Tatakai does **not** ship or copy:
//!
//! - the helper, next to the running executable (`tatakai-next`) or one folder above it (`temple-lsfg.exe` on
//!   Windows, `temple-lsfg` on Linux), or at the absolute path in `TEMPLE_LSFG` (development);
//! - the Lossless Scaling library (`Lossless.dll`: temple-lsfg reads the game's own DLL, unlike the lsfg-vk 2.x
//!   layer that wants its `lsfg-vk.dll` pack), given by the user's setting or searched in Steam: the default Steam
//!   folders and the libraries of their `steamapps/libraryfolders.vdf`. On Windows `C:\Program Files (x86)\Steam`;
//!   on Linux, in order, `$XDG_DATA_HOME/Steam` (or `~/.local/share/Steam`), `~/.steam/steam`, `~/.steam/root` and
//!   the Flatpak one (`~/.var/app/com.valvesoftware.Steam/.local/share/Steam`).
//!
//! At start, `temple-lsfg --probe --dll <path>` decides whether LSFG works on this GPU ([`detect`]); the result
//! (and the reason when not) goes to `/v1/capabilities` (`lsfg`, [`LsfgCapability`]) and gates the LSFG profiles.
//! Nothing here writes to disk or logs paths: reasons are codes.
//!
//! All system access (environment, files, the probe itself) goes through [`LsfgEnv`] and a closure, so it is tested
//! without touching the GPU or Steam.

use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::profiles::LSFG_MULTIPLIERS;

/// Absolute path of `temple-lsfg` (development override).
pub const HELPER_ENV: &str = "TEMPLE_LSFG";
/// Extra environment of every Vulkan process Link spawns: `temple-lsfg` (probe and sessions) and FFmpeg
/// (libplacebo / Anime4K use Vulkan). On Linux the implicit lsfg-vk Vulkan layer (when installed) loads into any
/// Vulkan process, reads its own config and writes errors to stderr even without a swapchain to hook:
/// `DISABLE_LSFGVK=1` keeps it out. Harmless on Windows.
pub const VULKAN_EXTRA_ENV: [(&str, &str); 1] = [("DISABLE_LSFGVK", "1")];
pub const HELPER_FILE_NAME: &str = if cfg!(windows) { "temple-lsfg.exe" } else { "temple-lsfg" };
pub const DLL_FILE_NAME: &str = "Lossless.dll";
pub const STEAM_DEFAULT_ROOT: &str = r"C:\Program Files (x86)\Steam";
/// The probe opens Vulkan and reads the library.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_VDF_BYTES: usize = 1024 * 1024;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// LSFG was not enabled by the host (`LinkConfig::lsfg`).
pub const REASON_DISABLED: &str = "disabled";
/// No processing engine (no FFmpeg).
pub const REASON_ENGINE: &str = "engine_unavailable";
/// `temple-lsfg` was not found.
pub const REASON_HELPER: &str = "helper_missing";
/// `Lossless.dll` was not found (the configured one or Steam's).
pub const REASON_DLL: &str = "dll_missing";
/// The probe could not run or was not understood.
pub const REASON_PROBE: &str = "probe_failed";
/// Error codes `temple-lsfg --probe` reports, passed through as-is.
const HELPER_ERRORS: [&str; 4] = ["dll_unreadable", "missing_spirv", "no_vulkan", "device_unsupported"];

/// `capabilities.lsfg` (additive field of Link v1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LsfgCapability {
    pub available: bool,
    /// LSFG on the PC is experimental.
    pub experimental: bool,
    pub multipliers: Vec<u8>,
    /// Target of the `*-auto120` profiles.
    pub auto_target_fps: u32,
    /// Why it is not available (a code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Helper and library LSFG is launched with. `Debug` hides the paths.
#[derive(Clone, PartialEq, Eq)]
pub struct LsfgPaths {
    pub helper: PathBuf,
    pub dll: PathBuf,
}

impl fmt::Debug for LsfgPaths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LsfgPaths").finish_non_exhaustive()
    }
}

/// What the probe says about this GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeInfo {
    pub device: String,
    pub fp16: bool,
}

/// A probe result: the GPU, or the reason code.
pub type ProbeResult = Result<ProbeInfo, String>;

/// Whether LSFG is available on this PC and with what. Computed once at start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LsfgStatus {
    paths: Option<LsfgPaths>,
    reason: Option<String>,
    device: Option<String>,
}

impl Default for LsfgStatus {
    fn default() -> Self {
        Self::unavailable(REASON_DISABLED)
    }
}

impl LsfgStatus {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self { paths: None, reason: Some(reason.into()), device: None }
    }

    pub fn available(paths: LsfgPaths, info: ProbeInfo) -> Self {
        Self { paths: Some(paths), reason: None, device: Some(info.device) }
    }

    pub fn is_available(&self) -> bool {
        self.paths.is_some()
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// GPU name from the probe, when available.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    pub fn paths(&self) -> Option<&LsfgPaths> {
        self.paths.as_ref()
    }

    pub fn capability(&self) -> LsfgCapability {
        LsfgCapability {
            available: self.is_available(),
            experimental: true,
            multipliers: LSFG_MULTIPLIERS.to_vec(),
            auto_target_fps: super::profiles::LSFG_AUTO_TARGET_FPS,
            reason: self.reason.clone(),
        }
    }
}

/// The system the helper and the library are searched in.
pub trait LsfgEnv {
    fn var(&self, name: &str) -> Option<OsString>;
    /// Folder of the running executable (where `temple-lsfg` lives, or its parent).
    fn exe_dir(&self) -> Option<PathBuf>;
    fn is_file(&self, path: &Path) -> bool;
    /// Contents of a small text file; `None` if missing or unreadable.
    fn read_to_string(&self, path: &Path) -> Option<String>;
    /// Default Steam folders, in preference order (they may not exist).
    fn steam_roots(&self) -> Vec<PathBuf>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemEnv;

impl LsfgEnv for SystemEnv {
    fn var(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }

    fn exe_dir(&self) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        exe.parent().map(Path::to_path_buf)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn steam_roots(&self) -> Vec<PathBuf> {
        if cfg!(windows) {
            vec![PathBuf::from(STEAM_DEFAULT_ROOT)]
        } else {
            linux_steam_roots(self.var("XDG_DATA_HOME").map(PathBuf::from).as_deref(), self.var("HOME").map(PathBuf::from).as_deref())
        }
    }
}

/// Default Linux Steam folders, in order and without repeats: `$XDG_DATA_HOME/Steam` (only when absolute; else
/// `$HOME/.local/share/Steam`), `$HOME/.steam/steam`, `$HOME/.steam/root` and the Flatpak one.
pub fn linux_steam_roots(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let xdg = xdg_data_home.filter(|path| path.is_absolute());
    let home = home.filter(|path| path.is_absolute());
    let mut roots = Vec::new();
    match (xdg, home) {
        (Some(xdg), _) => roots.push(xdg.join("Steam")),
        (None, Some(home)) => roots.push(home.join(".local").join("share").join("Steam")),
        (None, None) => {}
    }
    if let Some(home) = home {
        roots.push(home.join(".steam").join("steam"));
        roots.push(home.join(".steam").join("root"));
        roots.push(home.join(".var").join("app").join("com.valvesoftware.Steam").join(".local").join("share").join("Steam"));
    }
    let mut unique: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !unique.contains(&root) {
            unique.push(root);
        }
    }
    unique
}

/// The helper: the absolute path in `TEMPLE_LSFG` when set (and a file), else `temple-lsfg` next to the executable,
/// else one folder above. A `TEMPLE_LSFG` that does not work does not fall back: it is an explicit choice.
pub fn find_helper(env: &impl LsfgEnv) -> Option<PathBuf> {
    if let Some(path) = env.var(HELPER_ENV).filter(|value| !value.is_empty()) {
        let candidate = PathBuf::from(path);
        return (candidate.is_absolute() && env.is_file(&candidate)).then_some(candidate);
    }
    let dir = env.exe_dir()?;
    let beside = dir.join(HELPER_FILE_NAME);
    let above = dir.parent().map(|parent| parent.join(HELPER_FILE_NAME));
    std::iter::once(beside).chain(above).find(|candidate| candidate.is_absolute() && env.is_file(candidate))
}

/// Steam library paths of a `libraryfolders.vdf` (`"path"  "D:\\Games\\Steam"` on Windows,
/// `"path"  "/mnt/games/SteamLibrary"` on Linux), unescaped. Only absolute paths.
pub fn parse_library_folders(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let strings = quoted_strings(line.trim());
            let (key, value) = (strings.first()?, strings.get(1)?);
            if !key.eq_ignore_ascii_case("path") {
                return None;
            }
            let path = PathBuf::from(value);
            path.is_absolute().then_some(path)
        })
        .collect()
}

/// Quoted strings of a VDF line, with `\\` and `\"` unescaped.
fn quoted_strings(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut value = String::new();
        loop {
            match chars.next() {
                None | Some('"') => break,
                Some('\\') => match chars.next() {
                    Some('\\') => value.push('\\'),
                    Some('"') => value.push('"'),
                    Some(other) => {
                        value.push('\\');
                        value.push(other);
                    }
                    None => break,
                },
                Some(other) => value.push(other),
            }
        }
        found.push(value);
    }
    found
}

/// Where `Lossless.dll` can be in Steam: each default Steam folder followed by the libraries of its
/// `steamapps/libraryfolders.vdf`, in order and without repeats (several folders are often links to the same one).
pub fn steam_dll_candidates(env: &impl LsfgEnv) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in env.steam_roots() {
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if !roots.contains(&root) {
            roots.push(root);
        }
        let Some(text) = env.read_to_string(&vdf).filter(|text| text.len() <= MAX_VDF_BYTES) else {
            continue;
        };
        for library in parse_library_folders(&text) {
            if !roots.contains(&library) {
                roots.push(library);
            }
        }
    }
    roots.into_iter().map(|root| root.join("steamapps").join("common").join("Lossless Scaling").join(DLL_FILE_NAME)).collect()
}

/// The library: the configured path when there is one (absolute and a file; if it does not work no other is
/// searched: the user chose it) or the first existing [`steam_dll_candidates`].
pub fn find_dll(env: &impl LsfgEnv, configured: Option<&str>) -> Option<PathBuf> {
    match configured.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            (path.is_absolute() && env.is_file(&path)).then_some(path)
        }
        None => steam_dll_candidates(env).into_iter().find(|candidate| env.is_file(candidate)),
    }
}

/// Parse `temple-lsfg --probe` output: one JSON line `{"ok":true,"device":…,"fp16":…}` or
/// `{"ok":false,"error":"<code>"}`. `exit_ok` = the process exited with 0. Anything else is `probe_failed`.
pub fn parse_probe_output(stdout: &str, exit_ok: bool) -> ProbeResult {
    #[derive(Deserialize)]
    struct Line {
        ok: bool,
        device: Option<String>,
        fp16: Option<bool>,
        error: Option<String>,
    }
    let failed = || REASON_PROBE.to_owned();
    let line = stdout.lines().map(str::trim).find(|line| !line.is_empty()).ok_or_else(failed)?;
    let line: Line = serde_json::from_str(line).map_err(|_| failed())?;
    if line.ok {
        let device = line.device.ok_or_else(failed)?;
        if !exit_ok || device.chars().any(char::is_control) {
            return Err(failed());
        }
        return Ok(ProbeInfo { device, fp16: line.fp16.unwrap_or(false) });
    }
    match line.error {
        Some(code) if HELPER_ERRORS.contains(&code.as_str()) => Err(code),
        _ => Err(failed()),
    }
}

/// Find the helper and the library and probe them with `probe(helper, dll)`. The first missing piece is the
/// reported reason.
pub fn detect(env: &impl LsfgEnv, configured_dll: Option<&str>, probe: &dyn Fn(&Path, &Path) -> ProbeResult) -> LsfgStatus {
    let Some(helper) = find_helper(env) else {
        return LsfgStatus::unavailable(REASON_HELPER);
    };
    let Some(dll) = find_dll(env, configured_dll) else {
        return LsfgStatus::unavailable(REASON_DLL);
    };
    match probe(&helper, &dll) {
        Ok(info) => LsfgStatus::available(LsfgPaths { helper, dll }, info),
        Err(reason) => LsfgStatus::unavailable(reason),
    }
}

/// The real probe: `temple-lsfg --probe --dll <path>` (no console, [`PROBE_TIMEOUT`]), reading its single stdout
/// line. Blocking.
pub fn run_probe(helper: &Path, dll: &Path) -> ProbeResult {
    let failed = || REASON_PROBE.to_owned();
    let mut command = std::process::Command::new(helper);
    command.arg("--probe").arg("--dll").arg(dll).envs(VULKAN_EXTRA_ENV).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|err| {
        tracing::warn!(error = %err.kind(), "could not start the LSFG helper");
        failed()
    })?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                tracing::warn!("the LSFG probe timed out or could not be awaited");
                return Err(failed());
            }
        }
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        // one short line: the process already exited and the pipe did not fill up
        let _ = pipe.by_ref().take(16 * 1024).read_to_string(&mut stdout);
    }
    parse_probe_output(&stdout, status.success())
}

/// Full detection on the real system, for start-up: blocking (it launches the helper).
pub fn detect_system(configured_dll: Option<&str>) -> LsfgStatus {
    let status = detect(&SystemEnv, configured_dll, &run_probe);
    tracing::info!(available = status.is_available(), reason = status.reason().unwrap_or(""), "LSFG detection");
    status
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    fn abs(path: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!(r"Z:\{}", path.replace('/', "\\"))) } else { PathBuf::from(format!("/{path}")) }
    }

    #[derive(Default)]
    struct FakeEnv {
        vars: HashMap<String, OsString>,
        exe_dir: Option<PathBuf>,
        files: HashSet<PathBuf>,
        texts: HashMap<PathBuf, String>,
        steam: Vec<PathBuf>,
    }

    impl FakeEnv {
        fn new() -> Self {
            Self { steam: vec![abs("steam")], exe_dir: Some(abs("tatakai")), ..Self::default() }
        }

        fn with_file(mut self, path: PathBuf) -> Self {
            self.files.insert(path);
            self
        }

        fn with_text(mut self, path: PathBuf, text: &str) -> Self {
            self.files.insert(path.clone());
            self.texts.insert(path, text.to_owned());
            self
        }

        fn with_steam_roots(mut self, roots: &[&str]) -> Self {
            self.steam = roots.iter().map(|root| abs(root)).collect();
            self
        }

        fn with_var(mut self, name: &str, value: impl Into<OsString>) -> Self {
            self.vars.insert(name.to_owned(), value.into());
            self
        }

        fn steam_dll(root: &str) -> PathBuf {
            abs(root).join("steamapps").join("common").join("Lossless Scaling").join(DLL_FILE_NAME)
        }
    }

    impl LsfgEnv for FakeEnv {
        fn var(&self, name: &str) -> Option<OsString> {
            self.vars.get(name).cloned()
        }

        fn exe_dir(&self) -> Option<PathBuf> {
            self.exe_dir.clone()
        }

        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }

        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.texts.get(path).cloned()
        }

        fn steam_roots(&self) -> Vec<PathBuf> {
            self.steam.clone()
        }
    }

    fn helper_path() -> PathBuf {
        abs("tatakai").join(HELPER_FILE_NAME)
    }

    fn ok_probe(_: &Path, _: &Path) -> ProbeResult {
        Ok(ProbeInfo { device: "Fake GPU".to_owned(), fp16: true })
    }

    fn esc(p: &Path) -> String {
        p.display().to_string().replace('\\', "\\\\")
    }

    #[test]
    fn the_vdf_libraries_are_read_with_their_escapes() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"relative\\\\library\"\n\t}\n}\n";
        let libraries = parse_library_folders(vdf);
        if cfg!(windows) {
            assert_eq!(libraries, [PathBuf::from(r"C:\Program Files (x86)\Steam")]);
        } else {
            assert!(libraries.is_empty(), "no drive letters on Linux: nothing absolute");
        }
        assert!(parse_library_folders("").is_empty());
        assert!(parse_library_folders("garbage \"path\"").is_empty());
        assert_eq!(quoted_strings(r#""a\\b"  "c\"d""#), [r"a\b", "c\"d"]);
        assert_eq!(quoted_strings(r#""PATH"  "x"#), ["PATH", "x"]);
    }

    #[test]
    fn steam_candidates_start_with_the_default_root_and_add_the_libraries_once() {
        let root = abs("steam");
        let vdf = format!("\"libraryfolders\"\n{{\n\"0\"\n{{\n\"path\" \"{}\"\n}}\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n}}\n", esc(&root), esc(&abs("games/lib")));
        let env = FakeEnv::new().with_text(root.join("steamapps").join("libraryfolders.vdf"), &vdf);
        assert_eq!(steam_dll_candidates(&env), [FakeEnv::steam_dll("steam"), FakeEnv::steam_dll("games/lib")]);
        assert_eq!(steam_dll_candidates(&FakeEnv::new()), [FakeEnv::steam_dll("steam")]);
    }

    #[test]
    fn linux_default_roots_come_in_order_without_repeats() {
        let home = abs("home/ana");
        let xdg = abs("data");
        let flatpak = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        assert_eq!(linux_steam_roots(None, Some(&home)), [home.join(".local/share/Steam"), home.join(".steam/steam"), home.join(".steam/root"), flatpak]);
        assert_eq!(linux_steam_roots(Some(&xdg), Some(&home))[0], xdg.join("Steam"));
        assert_eq!(linux_steam_roots(Some(Path::new("relative")), Some(&home))[0], home.join(".local/share/Steam"));
        let same = home.join(".local/share");
        assert_eq!(linux_steam_roots(Some(&same), Some(&home)).len(), 4);
        assert_eq!(linux_steam_roots(Some(&xdg), None), [xdg.join("Steam")]);
        assert!(linux_steam_roots(None, None).is_empty());
        assert!(linux_steam_roots(None, Some(Path::new("home"))).is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn the_vdf_of_linux_has_plain_forward_slash_paths() {
        let vdf = concat!(
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n",
            "\t\t\"path\"\t\t\"/home/ana/.local/share/Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n",
            "\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/games/SteamLibrary\"\n\t}\n}\n",
        );
        assert_eq!(parse_library_folders(vdf), [PathBuf::from("/home/ana/.local/share/Steam"), PathBuf::from("/mnt/games/SteamLibrary")]);
        let env = FakeEnv::new()
            .with_steam_roots(&["home/ana/.local/share/Steam"])
            .with_text(abs("home/ana/.local/share/Steam/steamapps/libraryfolders.vdf"), vdf)
            .with_file(FakeEnv::steam_dll("mnt/games/SteamLibrary"));
        assert_eq!(find_dll(&env, None), Some(FakeEnv::steam_dll("mnt/games/SteamLibrary")));
    }

    #[test]
    fn several_roots_are_searched_in_order_and_libraries_are_deduplicated() {
        let vdf = format!("\"path\" \"{}\"\n", esc(&abs("mnt/lib")));
        let env = FakeEnv::new()
            .with_steam_roots(&["a/Steam", "b/steam", "c/flatpak"])
            .with_text(abs("a/Steam/steamapps/libraryfolders.vdf"), &vdf)
            .with_text(abs("b/steam/steamapps/libraryfolders.vdf"), &vdf)
            .with_text(abs("c/flatpak/steamapps/libraryfolders.vdf"), "");
        assert_eq!(
            steam_dll_candidates(&env),
            [FakeEnv::steam_dll("a/Steam"), FakeEnv::steam_dll("mnt/lib"), FakeEnv::steam_dll("b/steam"), FakeEnv::steam_dll("c/flatpak")]
        );
        let only_flatpak = env.with_file(FakeEnv::steam_dll("c/flatpak"));
        assert_eq!(find_dll(&only_flatpak, None), Some(FakeEnv::steam_dll("c/flatpak")));
    }

    #[test]
    fn the_dll_is_the_configured_one_or_the_first_found_in_steam() {
        let on_a_library = FakeEnv::steam_dll("games/lib");
        let vdf = format!("\"path\" \"{}\"\n", esc(&abs("games/lib")));
        let env = FakeEnv::new().with_text(abs("steam").join("steamapps").join("libraryfolders.vdf"), &vdf).with_file(on_a_library.clone());
        assert_eq!(find_dll(&env, None), Some(on_a_library.clone()));
        assert_eq!(find_dll(&env, Some("  ")), Some(on_a_library));
        let both = env.with_file(FakeEnv::steam_dll("steam"));
        assert_eq!(find_dll(&both, None), Some(FakeEnv::steam_dll("steam")));
        let chosen = abs("mine/Lossless.dll");
        let chosen_env = both.with_file(chosen.clone());
        assert_eq!(find_dll(&chosen_env, Some(&chosen.display().to_string())), Some(chosen));
        assert_eq!(find_dll(&chosen_env, Some(&abs("nowhere/Lossless.dll").display().to_string())), None);
        assert_eq!(find_dll(&chosen_env, Some("Lossless.dll")), None);
        assert_eq!(find_dll(&FakeEnv::new(), None), None);
    }

    #[test]
    fn the_helper_is_next_to_the_executable_or_in_the_environment_variable() {
        let beside = FakeEnv::new().with_file(helper_path());
        assert_eq!(find_helper(&beside), Some(helper_path()));
        assert_eq!(find_helper(&FakeEnv::new()), None);
        let dev = abs("dev/temple-lsfg");
        let env = beside.with_file(dev.clone()).with_var(HELPER_ENV, dev.clone().into_os_string());
        assert_eq!(find_helper(&env), Some(dev));
        let bad = FakeEnv::new().with_file(helper_path()).with_var(HELPER_ENV, abs("nowhere/temple-lsfg").into_os_string());
        assert_eq!(find_helper(&bad), None);
        let relative = FakeEnv::new().with_file(PathBuf::from("temple-lsfg")).with_var(HELPER_ENV, "temple-lsfg");
        assert_eq!(find_helper(&relative), None);
        // from a subfolder the helper of the parent folder is found; next to the executable wins
        let below = FakeEnv { exe_dir: Some(abs("tatakai/bin")), ..FakeEnv::new() }.with_file(helper_path());
        assert_eq!(find_helper(&below), Some(helper_path()));
        let both = FakeEnv { exe_dir: Some(abs("tatakai/bin")), ..FakeEnv::new() }.with_file(helper_path()).with_file(abs("tatakai/bin").join(HELPER_FILE_NAME));
        assert_eq!(find_helper(&both), Some(abs("tatakai/bin").join(HELPER_FILE_NAME)));
        let empty = FakeEnv::new().with_file(helper_path()).with_var(HELPER_ENV, "");
        assert_eq!(find_helper(&empty), Some(helper_path()));
        assert_eq!(find_helper(&FakeEnv { exe_dir: None, ..FakeEnv::new() }), None);
    }

    #[test]
    fn probe_output_maps_to_a_gpu_or_a_reason_code() {
        let ok = parse_probe_output(r#"{"ok":true,"device":"AMD Radeon RX 9070 XT (RADV GFX1201)","fp16":true}"#, true);
        assert_eq!(ok, Ok(ProbeInfo { device: "AMD Radeon RX 9070 XT (RADV GFX1201)".to_owned(), fp16: true }));
        assert!(!parse_probe_output("\n  {\"ok\":true,\"device\":\"GPU\"}\n", true).unwrap().fp16);
        for code in HELPER_ERRORS {
            assert_eq!(parse_probe_output(&format!(r#"{{"ok":false,"error":"{code}"}}"#), false), Err(code.to_owned()));
        }
        for (text, exit_ok) in [
            ("", true),
            ("not json", true),
            (r#"{"ok":false}"#, false),
            (r#"{"ok":false,"error":"boom"}"#, false),
            (r#"{"ok":true}"#, true),
            (r#"{"ok":true,"device":"GPU"}"#, false),
            (r#"{"ok":true,"device":"G\u0007PU"}"#, true),
            (r#"[1]"#, true),
        ] {
            assert_eq!(parse_probe_output(text, exit_ok), Err(REASON_PROBE.to_owned()), "{text}");
        }
    }

    #[test]
    fn detection_reports_the_first_missing_piece() {
        let dll = FakeEnv::steam_dll("steam");
        let status = detect(&FakeEnv::new().with_file(dll.clone()), None, &ok_probe);
        assert!(!status.is_available());
        assert_eq!(status.reason(), Some(REASON_HELPER));
        assert!(status.paths().is_none());
        assert_eq!(detect(&FakeEnv::new().with_file(helper_path()), None, &ok_probe).reason(), Some(REASON_DLL));
        let env = FakeEnv::new().with_file(helper_path()).with_file(dll.clone());
        let status = detect(&env, None, &ok_probe);
        assert!(status.is_available());
        assert_eq!(status.reason(), None);
        assert_eq!(status.device(), Some("Fake GPU"));
        let paths = status.paths().unwrap();
        assert_eq!((&paths.helper, &paths.dll), (&helper_path(), &dll));
        let seen = std::cell::RefCell::new(None);
        let spy = |helper: &Path, dll: &Path| {
            *seen.borrow_mut() = Some((helper.to_path_buf(), dll.to_path_buf()));
            ok_probe(helper, dll)
        };
        detect(&env, None, &spy);
        assert_eq!(seen.into_inner(), Some((helper_path(), dll)));
        for code in HELPER_ERRORS {
            let failing = |_: &Path, _: &Path| -> ProbeResult { Err(code.to_owned()) };
            assert_eq!(detect(&env, None, &failing).reason(), Some(code));
        }
    }

    #[test]
    fn capability_is_experimental_and_only_carries_a_reason_when_unavailable() {
        let env = FakeEnv::new().with_file(helper_path()).with_file(FakeEnv::steam_dll("steam"));
        assert_eq!(
            serde_json::to_value(detect(&env, None, &ok_probe).capability()).unwrap(),
            serde_json::json!({ "available": true, "experimental": true, "multipliers": [2, 3, 4, 5], "auto_target_fps": 120 })
        );
        let missing = detect(&FakeEnv::new(), None, &ok_probe).capability();
        assert_eq!(
            serde_json::to_value(&missing).unwrap(),
            serde_json::json!({ "available": false, "experimental": true, "multipliers": [2, 3, 4, 5], "auto_target_fps": 120, "reason": "helper_missing" })
        );
        assert_eq!(LsfgStatus::default().reason(), Some(REASON_DISABLED));
    }

    #[test]
    fn paths_never_show_in_debug() {
        let paths = LsfgPaths { helper: abs("secret-user/temple-lsfg"), dll: abs("secret-user/Lossless.dll") };
        assert!(!format!("{paths:?}").contains("secret-user"));
        let status = LsfgStatus::available(paths, ProbeInfo { device: "GPU".to_owned(), fp16: false });
        assert!(!format!("{status:?}").contains("secret-user"));
    }

    #[test]
    fn a_missing_helper_makes_the_real_probe_fail_cleanly() {
        assert_eq!(run_probe(&abs("nowhere/temple-lsfg"), &abs("nowhere/Lossless.dll")), Err(REASON_PROBE.to_owned()));
    }
}
