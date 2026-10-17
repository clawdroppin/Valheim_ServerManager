//! Server-side mods: BepInEx (the `denikson-BepInExPack_Valheim` package every Valheim mod builds
//! on) and mods from Thunderstore, the community's mod site.
//!
//! Layout (verified against BepInExPack_Valheim 5.4.2351): the pack's `BepInExPack_Valheim/`
//! folder goes into the server folder – `winhttp.dll` (the Doorstop proxy Windows loads next to
//! `valheim_server.exe`), `doorstop_config.ini`, `BepInEx\core`, `BepInEx\config\BepInEx.cfg`.
//! Mods are Thunderstore zips with `manifest.json` and their files under `plugins/` (or `config/`,
//! `patchers/`, optionally inside `BepInEx/`); they go into `BepInEx\plugins\<Namespace-Name>\`,
//! the folder layout r2modman uses, so its installs are recognised too.
//!
//! "Mods off" parks `winhttp.dll` as `winhttp.dll.disabled` (the server then starts vanilla);
//! a disabled mod moves to `BepInEx\plugins_disabled\`. Everything that changes files needs the
//! server stopped – it has the DLLs loaded.

use super::download::download;
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::state::TaskReporter;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

const PROXY: &str = "winhttp.dll";
const PROXY_OFF: &str = "winhttp.dll.disabled";
const BEPINEX_PACK: &str = "denikson-BepInExPack_Valheim";
const API: &str = "https://thunderstore.io/api/experimental/package";

pub fn bepinex_installed(inst: &ServerInstance) -> bool {
    inst.bepinex_dir().join("core").join("BepInEx.dll").exists()
}

fn disabled_dir(inst: &ServerInstance) -> PathBuf {
    inst.bepinex_dir().join("plugins_disabled")
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    /// Folder or file name under `plugins` / `plugins_disabled` – the handle the UI uses.
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub website: Option<String>,
    /// `Namespace-Name` when it came from Thunderstore (or looks like it).
    pub package: Option<String>,
    pub dependencies: Vec<String>,
    pub enabled: bool,
    pub dlls: u32,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModConfig {
    pub name: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsStatus {
    pub bepinex: bool,
    pub bepinex_version: Option<String>,
    /// The Doorstop proxy is in place (mods load at the next start).
    pub proxy_active: bool,
    pub mods: Vec<ModInfo>,
    pub configs: Vec<ModConfig>,
    /// `BepInEx\LogOutput.log` exists (written by the last modded run).
    pub has_log: bool,
}

fn mtime(p: &Path) -> i64 {
    std::fs::metadata(p).and_then(|m| m.modified()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()).unwrap_or(0)
}

/// `manifest.json` of a Thunderstore package (r2modman writes the same file next to the mod).
fn read_manifest(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read(dir.join("manifest.json")).ok()?;
    serde_json::from_str(String::from_utf8_lossy(&text).trim_start_matches('\u{feff}')).ok()
}

fn mod_from(path: &Path, enabled: bool) -> Option<ModInfo> {
    let id = path.file_name()?.to_string_lossy().into_owned();
    if id.starts_with('.') {
        return None;
    }
    let mut m = ModInfo { id: id.clone(), name: id.clone(), enabled, modified: mtime(path), ..Default::default() };
    if path.is_dir() {
        for e in walkdir::WalkDir::new(path).into_iter().flatten().filter(|e| e.file_type().is_file()) {
            m.size += e.metadata().map(|x| x.len()).unwrap_or(0);
            if e.path().extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                m.dlls += 1;
            }
        }
        if let Some(v) = read_manifest(path) {
            let s = |k: &str| v[k].as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
            m.name = s("name").map(|n| n.replace('_', " ")).unwrap_or(m.name);
            m.version = s("version_number");
            m.description = s("description");
            m.website = s("website_url");
            m.dependencies = v["dependencies"].as_array().map(|a| a.iter().filter_map(|d| d.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        }
        // r2modman / our folders are `Namespace-Name`.
        if let Some((ns, name)) = id.split_once('-') {
            if !ns.is_empty() && !name.is_empty() && !name.contains('-') {
                m.author = Some(ns.to_string());
                m.package = Some(id.clone());
            }
        }
    } else {
        if !id.to_lowercase().ends_with(".dll") {
            return None;
        }
        m.dlls = 1;
        m.size = std::fs::metadata(path).map(|x| x.len()).unwrap_or(0);
        m.name = id.trim_end_matches(".dll").trim_end_matches(".DLL").to_string();
    }
    Some(m)
}

pub fn status(inst: &ServerInstance) -> ModsStatus {
    let bepinex = bepinex_installed(inst);
    let mut mods = vec![];
    for (dir, enabled) in [(inst.plugins_dir(), true), (disabled_dir(inst), false)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if let Some(m) = mod_from(&e.path(), enabled) {
                mods.push(m);
            }
        }
    }
    mods.sort_by_key(|m| m.name.to_lowercase());
    let mut configs: Vec<ModConfig> = std::fs::read_dir(inst.bepinex_dir().join("config"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("cfg") || x.eq_ignore_ascii_case("json") || x.eq_ignore_ascii_case("yml") || x.eq_ignore_ascii_case("yaml")).unwrap_or(false))
                .map(|e| ModConfig { name: e.file_name().to_string_lossy().into_owned(), size: e.metadata().map(|m| m.len()).unwrap_or(0), modified: mtime(&e.path()) })
                .collect()
        })
        .unwrap_or_default();
    configs.sort_by_key(|c| c.name.to_lowercase());
    ModsStatus {
        bepinex,
        bepinex_version: std::fs::read_to_string(inst.install_dir.join(".bepinexpack_version")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        proxy_active: inst.install_dir.join(PROXY).exists(),
        mods,
        configs,
        has_log: inst.bepinex_dir().join("LogOutput.log").exists(),
    }
}

/// Put the Doorstop proxy in place or park it, following the profile's "mods" switch. Called just
/// before every start. Returns a line for the console when something changed.
pub fn apply_enabled(inst: &ServerInstance) -> AppResult<Option<String>> {
    if !bepinex_installed(inst) {
        return Ok(None);
    }
    let on = inst.install_dir.join(PROXY);
    let off = inst.install_dir.join(PROXY_OFF);
    if inst.launch.mods_enabled && !on.exists() && off.exists() {
        std::fs::rename(&off, &on)?;
        return Ok(Some("Mods are on – BepInEx loads with the server.".into()));
    }
    if !inst.launch.mods_enabled && on.exists() {
        if off.exists() {
            std::fs::remove_file(&off)?;
        }
        std::fs::rename(&on, &off)?;
        return Ok(Some("Mods are off for this start – the server runs vanilla.".into()));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------------- Thunderstore

/// `Namespace-Name` from what a user pastes: `Namespace-Name`, `Namespace/Name`,
/// `Namespace-Name-1.2.3` (a dependency string) or a thunderstore.io package URL.
pub fn parse_spec(spec: &str) -> Option<(String, String)> {
    let s = spec.trim().trim_end_matches('/');
    let s = if let Some(i) = s.find("/p/") {
        &s[i + 3..]
    } else if let Some(i) = s.find("/package/") {
        let rest = &s[i + 9..];
        rest.strip_prefix("download/").unwrap_or(rest)
    } else {
        s
    };
    let parts: Vec<&str> = s.split(['/', '-']).filter(|p| !p.is_empty()).collect();
    let ok = |p: &str| p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    match parts.as_slice() {
        [ns, name, ..] if ok(ns) && ok(name) => Some((ns.to_string(), name.to_string())),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub full_name: String,
    pub version: String,
    pub description: String,
    pub download_url: String,
    pub dependencies: Vec<String>,
    pub website: String,
    pub deprecated: bool,
}

pub async fn package_info(http: &reqwest::Client, ns: &str, name: &str) -> AppResult<PackageInfo> {
    let url = format!("{API}/{ns}/{name}/");
    let resp = http.get(&url).timeout(std::time::Duration::from_secs(15)).send().await?;
    if resp.status().as_u16() == 404 {
        return Err(AppError::msg(format!("Thunderstore has no package “{ns}-{name}” – copy the name from the mod's Thunderstore page.")));
    }
    let v: serde_json::Value = resp.error_for_status()?.json().await?;
    let l = &v["latest"];
    let s = |x: &serde_json::Value| x.as_str().unwrap_or_default().to_string();
    Ok(PackageInfo {
        full_name: format!("{}-{}", s(&v["namespace"]), s(&v["name"])),
        version: s(&l["version_number"]),
        description: s(&l["description"]),
        download_url: s(&l["download_url"]),
        dependencies: l["dependencies"].as_array().map(|a| a.iter().filter_map(|d| d.as_str().map(|x| x.to_string())).collect()).unwrap_or_default(),
        website: s(&l["website_url"]),
        deprecated: v["is_deprecated"].as_bool().unwrap_or(false),
    })
}

/// Where a file from a mod zip goes (relative to `BepInEx`), r2modman style.
fn route(rel: &str, package: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let rel = rel.strip_prefix("BepInEx/").unwrap_or(&rel);
    let lower = rel.to_lowercase();
    // Package metadata stays with the mod (manifest.json identifies it later).
    if ["readme.md", "changelog.md", "icon.png", "manifest.json"].contains(&lower.as_str()) {
        return Some(PathBuf::from("plugins").join(package).join(rel));
    }
    let (first, rest) = rel.split_once('/').unwrap_or(("", rel));
    let p = match first.to_lowercase().as_str() {
        "plugins" => PathBuf::from("plugins").join(package).join(rest),
        "patchers" => PathBuf::from("patchers").join(package).join(rest),
        "config" => PathBuf::from("config").join(rest),
        "core" => return None, // never let a mod replace BepInEx itself
        _ => PathBuf::from("plugins").join(package).join(rel),
    };
    Some(p)
}

/// Unpack a Thunderstore mod zip into `BepInEx`. Config files that already exist are kept (the
/// user's settings win over the mod's defaults). Returns the number of files written.
pub fn unpack_mod(inst: &ServerInstance, zip_path: &Path, package: &str) -> AppResult<usize> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f).map_err(|_| AppError::msg("That file isn't a zip archive."))?;
    let target = inst.plugins_dir().join(package);
    // An update replaces the old files of the mod (kept aside, not deleted).
    if target.exists() {
        let aside = inst.install_dir.join("_mods-removed").join(format!("{}-{package}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::create_dir_all(aside.parent().unwrap())?;
        std::fs::rename(&target, &aside)?;
    }
    let _ = std::fs::remove_dir_all(disabled_dir(inst).join(package));
    let base = inst.bepinex_dir();
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        if e.is_dir() {
            continue;
        }
        let Some(name) = e.enclosed_name() else { continue };
        let Some(rel) = route(&name.to_string_lossy(), package) else { continue };
        let out = base.join(rel);
        if !super::proc::is_within(&out, &base) {
            continue;
        }
        if out.starts_with(base.join("config")) && out.exists() {
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut buf = vec![];
        e.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
        n += 1;
    }
    if n == 0 {
        return Err(AppError::msg("The archive contained no mod files."));
    }
    Ok(n)
}

/// Download and unpack BepInExPack_Valheim into the server folder.
pub async fn install_bepinex(http: &reqwest::Client, inst: &ServerInstance, tmp: &Path, r: &TaskReporter) -> AppResult<String> {
    let info = package_info(http, "denikson", "BepInExPack_Valheim").await?;
    let zip = tmp.join(format!("bepinex-{}.zip", uuid::Uuid::new_v4()));
    download(http, &info.download_url, &zip, Some(r), "BepInExPack_Valheim").await?;
    r.progress("Unpacking BepInEx", None);
    let dest = inst.install_dir.clone();
    let z2 = zip.clone();
    let files = tokio::task::spawn_blocking(move || -> AppResult<usize> {
        let f = std::fs::File::open(&z2)?;
        let mut z = zip::ZipArchive::new(f)?;
        let mut n = 0;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_dir() {
                continue;
            }
            let Some(name) = e.enclosed_name() else { continue };
            let s = name.to_string_lossy().replace('\\', "/");
            let Some(rel) = s.strip_prefix("BepInExPack_Valheim/") else { continue };
            let out = dest.join(rel);
            // Keep the user's BepInEx.cfg on a reinstall.
            if rel.starts_with("BepInEx/config/") && out.exists() {
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut buf = vec![];
            e.read_to_end(&mut buf)?;
            std::fs::write(&out, buf)?;
            n += 1;
        }
        // A parked proxy from "mods off" would otherwise sit next to the fresh one.
        let _ = std::fs::remove_file(dest.join(PROXY_OFF));
        std::fs::create_dir_all(dest.join("BepInEx").join("plugins"))?;
        Ok(n)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    let _ = tokio::fs::remove_file(&zip).await;
    if files == 0 {
        return Err(AppError::msg("The BepInEx package had an unexpected layout – nothing was installed."));
    }
    let _ = std::fs::write(inst.install_dir.join(".bepinexpack_version"), &info.version);
    Ok(info.version)
}

/// Install a Thunderstore package and its dependencies (BepInEx itself is installed when missing).
/// Returns the packages installed, dependencies first.
pub async fn install_package(http: &reqwest::Client, inst: &ServerInstance, tmp: &Path, spec: &str, r: &TaskReporter) -> AppResult<Vec<String>> {
    let (ns, name) = parse_spec(spec).ok_or_else(|| AppError::msg(format!("“{}” isn't a Thunderstore package – paste its page URL or Namespace-Name.", spec.trim())))?;
    if format!("{ns}-{name}") == BEPINEX_PACK {
        let v = install_bepinex(http, inst, tmp, r).await?;
        return Ok(vec![format!("{BEPINEX_PACK}-{v}")]);
    }
    if !bepinex_installed(inst) {
        r.progress("Installing BepInEx first", None);
        install_bepinex(http, inst, tmp, r).await?;
    }
    // Resolve the dependency tree breadth-first, then install leaves first.
    let mut order: Vec<PackageInfo> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue = vec![(ns, name)];
    while let Some((ns, name)) = queue.pop() {
        let key = format!("{ns}-{name}");
        if key == BEPINEX_PACK || !seen.insert(key.clone()) {
            continue;
        }
        if seen.len() > 60 {
            return Err(AppError::msg("That mod pulls in more than 60 packages – install it with a mod manager instead."));
        }
        r.progress(&format!("Looking up {key}"), None);
        let info = package_info(http, &ns, &name).await?;
        for d in &info.dependencies {
            if let Some(p) = parse_spec(d) {
                queue.push(p);
            }
        }
        order.push(info);
    }
    // The requested package was resolved first; install dependencies before it.
    let requested = order.first().map(|p| p.full_name.clone()).unwrap_or_default();
    order.reverse();
    let mut done = vec![];
    for info in order {
        // Dependencies already installed at the same version (enabled or not) are left alone; the
        // requested package is always (re)installed.
        let installed_version = [inst.plugins_dir(), disabled_dir(inst)]
            .iter()
            .find_map(|d| read_manifest(&d.join(&info.full_name)).and_then(|v| v["version_number"].as_str().map(|s| s.to_string())));
        if info.full_name != requested && installed_version.as_deref() == Some(info.version.as_str()) {
            continue;
        }
        let zip = tmp.join(format!("{}-{}.zip", info.full_name, uuid::Uuid::new_v4()));
        download(http, &info.download_url, &zip, Some(r), &info.full_name).await?;
        let (i2, z2, pkg) = (inst.clone(), zip.clone(), info.full_name.clone());
        tokio::task::spawn_blocking(move || unpack_mod(&i2, &z2, &pkg)).await.map_err(|e| AppError::msg(e.to_string()))??;
        let _ = tokio::fs::remove_file(&zip).await;
        done.push(format!("{}-{}", info.full_name, info.version));
    }
    Ok(done)
}

/// Install a mod from a file: a Thunderstore-style zip, or a single plugin DLL.
pub fn install_file(inst: &ServerInstance, path: &Path) -> AppResult<String> {
    if !bepinex_installed(inst) {
        return Err(AppError::msg("Install BepInEx first – mods need it."));
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "mod".into());
    let ext = path.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "dll" {
        let dir = inst.plugins_dir().join(&stem);
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(path, dir.join(path.file_name().unwrap()))?;
        return Ok(stem);
    }
    if ext != "zip" {
        return Err(AppError::msg("Pick a mod .zip (as downloaded from Thunderstore) or a plugin .dll."));
    }
    // Name it like Thunderstore would when the zip carries a manifest.
    let package = (|| {
        let f = std::fs::File::open(path).ok()?;
        let mut z = zip::ZipArchive::new(f).ok()?;
        let mut e = z.by_name("manifest.json").ok()?;
        let mut s = String::new();
        e.read_to_string(&mut s).ok()?;
        let v: serde_json::Value = serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()?;
        let name = v["name"].as_str()?.to_string();
        // "Author-Name-1.0.0.zip" (Thunderstore's download name) carries the namespace.
        let ns = stem.split('-').next().filter(|n| !n.is_empty() && *n != name).map(|n| n.to_string());
        Some(match ns {
            Some(ns) => format!("{ns}-{name}"),
            None => name,
        })
    })()
    .unwrap_or_else(|| stem.clone());
    unpack_mod(inst, path, &package)?;
    Ok(package)
}

fn mod_path(inst: &ServerInstance, id: &str) -> AppResult<(PathBuf, bool)> {
    if id.is_empty() || id.contains(['\\', '/', ':']) || id.starts_with('.') {
        return Err(AppError::msg("Invalid mod"));
    }
    let on = inst.plugins_dir().join(id);
    if on.exists() {
        return Ok((on, true));
    }
    let off = disabled_dir(inst).join(id);
    if off.exists() {
        return Ok((off, false));
    }
    Err(AppError::msg(format!("Mod “{id}” wasn't found.")))
}

pub fn set_enabled(inst: &ServerInstance, id: &str, enabled: bool) -> AppResult<()> {
    let (p, is_on) = mod_path(inst, id)?;
    if is_on == enabled {
        return Ok(());
    }
    let target = if enabled { inst.plugins_dir().join(id) } else { disabled_dir(inst).join(id) };
    std::fs::create_dir_all(target.parent().unwrap())?;
    std::fs::rename(&p, &target).map_err(|e| AppError::msg(format!("Couldn't move the mod ({e}) – is the server still running?")))?;
    Ok(())
}

/// Move a mod out of BepInEx (into `_mods-removed`, recoverable). Its config file stays.
pub fn remove(inst: &ServerInstance, id: &str) -> AppResult<()> {
    let (p, _) = mod_path(inst, id)?;
    let aside = inst.install_dir.join("_mods-removed").join(format!("{}-{id}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(aside.parent().unwrap())?;
    std::fs::rename(&p, &aside).map_err(|e| AppError::msg(format!("Couldn't remove the mod ({e}) – is the server still running?")))?;
    Ok(())
}

/// Take BepInEx out of the server folder (kept in `_mods-removed`): the server is vanilla again.
pub fn uninstall_bepinex(inst: &ServerInstance) -> AppResult<PathBuf> {
    let aside = inst.install_dir.join("_mods-removed").join(format!("{}-BepInEx", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&aside)?;
    for n in [PROXY, PROXY_OFF, "doorstop_config.ini", ".doorstop_version", "BepInEx", "doorstop_libs", "unstripped_corlib", "start_game_bepinex.sh", "start_server_bepinex.sh", "changelog.txt", ".bepinexpack_version"] {
        let p = inst.install_dir.join(n);
        if p.exists() {
            std::fs::rename(&p, aside.join(n)).map_err(|e| AppError::msg(format!("Couldn't move {n} ({e}) – is the server still running?")))?;
        }
    }
    Ok(aside)
}

pub fn config_path(inst: &ServerInstance, name: &str) -> AppResult<PathBuf> {
    if name.is_empty() || name.contains(['\\', '/', ':']) || name.starts_with('.') {
        return Err(AppError::msg("Invalid config file"));
    }
    Ok(inst.bepinex_dir().join("config").join(name))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub id: String,
    pub installed: Option<String>,
    pub latest: String,
}

/// Newer Thunderstore versions for the installed packages.
pub async fn check_updates(http: &reqwest::Client, inst: &ServerInstance) -> Vec<ModUpdate> {
    let mut out = vec![];
    for m in status(inst).mods.into_iter().filter(|m| m.package.is_some()) {
        let Some((ns, name)) = m.package.as_deref().and_then(parse_spec) else { continue };
        if let Ok(info) = package_info(http, &ns, &name).await {
            if m.version.as_deref() != Some(info.version.as_str()) {
                out.push(ModUpdate { id: m.id, installed: m.version, latest: info.version });
            }
        }
    }
    out
}

/// The last lines of `BepInEx\LogOutput.log` (plugin load errors show up here).
pub fn log_tail(inst: &ServerInstance, lines: usize) -> Vec<String> {
    let text = std::fs::read(inst.bepinex_dir().join("LogOutput.log")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn specs_from_names_and_urls() {
        let j = Some(("ValheimModding".to_string(), "Jotunn".to_string()));
        assert_eq!(parse_spec("ValheimModding-Jotunn"), j);
        assert_eq!(parse_spec("ValheimModding/Jotunn"), j);
        assert_eq!(parse_spec("ValheimModding-Jotunn-2.30.2"), j);
        assert_eq!(parse_spec("https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/"), j);
        assert_eq!(parse_spec("https://thunderstore.io/package/download/ValheimModding/Jotunn/2.30.2/"), j);
        assert_eq!(parse_spec("nonsense"), None);
        assert_eq!(parse_spec("a b-c"), None);
    }

    #[test]
    fn routes_like_r2modman() {
        let r = |s: &str| route(s, "A-B").map(|p| p.to_string_lossy().replace('\\', "/"));
        assert_eq!(r("plugins/Jotunn.dll").as_deref(), Some("plugins/A-B/Jotunn.dll"));
        assert_eq!(r("BepInEx/plugins/x/y.dll").as_deref(), Some("plugins/A-B/x/y.dll"));
        assert_eq!(r("config/a.cfg").as_deref(), Some("config/a.cfg"));
        assert_eq!(r("Mod.dll").as_deref(), Some("plugins/A-B/Mod.dll"));
        assert_eq!(r("manifest.json").as_deref(), Some("plugins/A-B/manifest.json"));
        assert_eq!(r("BepInEx/core/BepInEx.dll"), None);
    }

    fn zip_with(path: &Path, files: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (n, b) in files {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn install_toggle_remove_and_proxy() {
        let d = std::env::temp_dir().join(format!("vhsm-mods-{}", uuid::Uuid::new_v4()));
        let mut inst = ServerInstance { install_dir: d.clone(), ..Default::default() };
        std::fs::create_dir_all(inst.bepinex_dir().join("core")).unwrap();
        std::fs::write(inst.bepinex_dir().join("core").join("BepInEx.dll"), b"x").unwrap();
        std::fs::write(d.join(PROXY), b"proxy").unwrap();
        std::fs::create_dir_all(inst.bepinex_dir().join("config")).unwrap();
        std::fs::write(inst.bepinex_dir().join("config").join("mine.cfg"), b"user=1").unwrap();
        let z = d.join("ValheimModding-Jotunn-2.30.2.zip");
        zip_with(&z, &[("manifest.json", br#"{"name":"Jotunn","version_number":"2.30.2","description":"lib","dependencies":["denikson-BepInExPack_Valheim-5.4.2333"]}"#), ("plugins/Jotunn.dll", b"dll"), ("config/mine.cfg", b"default=1")]);
        assert_eq!(install_file(&inst, &z).unwrap(), "ValheimModding-Jotunn");
        let s = status(&inst);
        assert_eq!(s.mods.len(), 1);
        let m = &s.mods[0];
        assert_eq!((m.name.as_str(), m.version.as_deref(), m.dlls, m.enabled), ("Jotunn", Some("2.30.2"), 1, true));
        assert_eq!(m.package.as_deref(), Some("ValheimModding-Jotunn"));
        assert_eq!(std::fs::read(inst.bepinex_dir().join("config").join("mine.cfg")).unwrap(), b"user=1", "user config kept");
        set_enabled(&inst, "ValheimModding-Jotunn", false).unwrap();
        assert!(!status(&inst).mods[0].enabled);
        set_enabled(&inst, "ValheimModding-Jotunn", true).unwrap();
        assert!(set_enabled(&inst, "../x", true).is_err());
        // Mods off parks the proxy; on brings it back.
        inst.launch.mods_enabled = false;
        assert!(apply_enabled(&inst).unwrap().is_some());
        assert!(!d.join(PROXY).exists() && d.join(PROXY_OFF).exists());
        inst.launch.mods_enabled = true;
        assert!(apply_enabled(&inst).unwrap().is_some());
        assert!(d.join(PROXY).exists());
        assert!(apply_enabled(&inst).unwrap().is_none());
        remove(&inst, "ValheimModding-Jotunn").unwrap();
        assert!(status(&inst).mods.is_empty());
        uninstall_bepinex(&inst).unwrap();
        assert!(!bepinex_installed(&inst) && !d.join(PROXY).exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
