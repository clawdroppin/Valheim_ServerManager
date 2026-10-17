//! Worlds in `<savedir>\worlds_local`.
//!
//! Valheim 1.0.20 (verified, Steam build 25909431) keeps every world in a folder of its own:
//! `worlds_local\<World>\_main.<N>.fwl2` (metadata: name, seed, world modifiers),
//! `_main.<N>.db2` (int32 version, double world time, gzip data), `_main.<N>.chunks`,
//! `_main.<N>.ok`, plus `XX_YY__<a>_<N>.chunk` files that are only rewritten when dirty. Every save
//! bumps N and removes the previous `_main` set, so a world folder only ever holds one save.
//! Older versions used a file pair `worlds_local\<World>.fwl` + `<World>.db` (+ `.old` copies); those
//! are listed too and the server converts them on load ("save from an old save format, created
//! backup!").
//!
//! The server's own backups (`-backups/-backupshort/-backuplong`) are named
//! `<World>_backup_auto-<yyyyMMdd-HHmmss>` next to the world (format from the binary's strings – a
//! backup wasn't produced during testing). Rolling back copies one over the world after moving the
//! current world into `<savedir>\_quarantine`.
//!
//! The world's name is stored inside its `.fwl2`/`.fwl`, and the server saves under that name – so
//! renaming or duplicating a world rewrites it there, not just the folder name.

use crate::error::{AppError, AppResult};
use crate::models::*;
use regex::Regex;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

static MAIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^_main\.(\d+)\.fwl2$").unwrap());
static BACKUP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.+?)_backup_(?:([a-z]+)-)?(\d{8}-?\d{6})?").unwrap());

fn ms(t: SystemTime) -> i64 {
    chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()
}

fn mtime(p: &Path) -> i64 {
    std::fs::metadata(p).and_then(|m| m.modified()).map(ms).unwrap_or(0)
}

fn dir_size(p: &Path) -> u64 {
    walkdir::WalkDir::new(p).into_iter().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

// ------------------------------------------------------------------------------- file formats

/// What a `.fwl2` / `.fwl` says about the world.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldMeta {
    pub name: String,
    pub seed_name: String,
    pub seed: i32,
    pub version: i32,
    /// Starting global keys: `preset combat_hard:deathpenalty_default:…`, `nomap`, `playerdamage 85` …
    pub keys: Vec<String>,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    /// .NET BinaryWriter string: 7-bit encoded length + UTF-8.
    fn string(&mut self) -> Option<String> {
        let mut len = 0usize;
        for shift in (0..35).step_by(7) {
            let b = self.u8()?;
            len |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
        }
        if len > 4096 {
            return None;
        }
        self.take(len).map(|s| String::from_utf8_lossy(s).into_owned())
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let mut n = s.len();
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(s.as_bytes());
}

/// Layout (verified against real files): int32 payload length, then int32 version, string name,
/// string seed name, int32 seed, int64 uid, int32 world-gen version, bool, int32 key count, keys.
pub fn parse_meta(bytes: &[u8]) -> Option<WorldMeta> {
    let mut r = Reader { b: bytes, i: 0 };
    let _len = r.i32()?;
    let version = r.i32()?;
    let name = r.string()?;
    let seed_name = r.string()?;
    let seed = r.i32()?;
    let mut m = WorldMeta { name, seed_name, seed, version, keys: vec![] };
    // Optional tail – an older/newer layout just yields no keys.
    let tail = (|| {
        r.take(8)?; // uid
        r.i32()?; // world-gen version
        r.u8()?;
        let n = r.i32()?;
        if !(0..=500).contains(&n) {
            return None;
        }
        let mut keys = vec![];
        for _ in 0..n {
            keys.push(r.string()?);
        }
        Some(keys)
    })();
    if let Some(k) = tail {
        m.keys = k;
    }
    Some(m)
}

/// The same file with another world name (payload length updated, everything else byte-identical).
pub fn rename_meta(bytes: &[u8], new_name: &str) -> Option<Vec<u8>> {
    let mut r = Reader { b: bytes, i: 0 };
    let _len = r.i32()?;
    let version = r.i32()?;
    let _ = r.string()?;
    let rest = &bytes[r.i..];
    let mut payload = version.to_le_bytes().to_vec();
    write_string(&mut payload, new_name);
    payload.extend_from_slice(rest);
    let mut out = (payload.len() as i32).to_le_bytes().to_vec();
    out.extend(payload);
    Some(out)
}

/// World time (seconds) at the start of a `.db2` / `.db`: int32 version, double time (verified:
/// a fresh world starts at 2040.0 – day 1 – and time only runs while players are online).
pub fn read_world_time(path: &Path) -> Option<f64> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = [0u8; 12];
    f.read_exact(&mut h).ok()?;
    let t = f64::from_le_bytes([h[4], h[5], h[6], h[7], h[8], h[9], h[10], h[11]]);
    (t.is_finite() && t >= 0.0).then_some(t)
}

pub fn day_of(time: f64) -> u32 {
    (time / DAY_SECS).floor().max(0.0) as u32
}

// ---------------------------------------------------------------------------------- the model

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldBackup {
    /// Folder or file stem, e.g. `Dedicated_backup_auto-20261016-081500`.
    pub name: String,
    /// auto | restore | cloud | manual
    pub kind: String,
    pub created_at: i64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct World {
    pub name: String,
    /// folder (1.0.20+) | legacy (`.fwl` + `.db`)
    pub format: String,
    /// The world the server loads (`-world`).
    pub active: bool,
    /// N of `_main.<N>` (folder worlds).
    pub save_number: Option<u32>,
    pub meta: Option<WorldMeta>,
    pub day: Option<u32>,
    pub saved_at: Option<i64>,
    pub size: u64,
    pub path: PathBuf,
    pub backups: Vec<WorldBackup>,
    /// Legacy worlds: a `.db.old` from the previous save exists.
    pub has_old: bool,
}

/// The save the server would load: the newest `_main.N` set that is complete (its `.ok` marker is
/// written last – verified order: chunks, db2, fwl2, then ok), else the newest at all.
pub fn latest_main(dir: &Path) -> Option<u32> {
    let mut ns: Vec<u32> = std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| MAIN_RE.captures(&e.file_name().to_string_lossy()).and_then(|c| c[1].parse().ok())).collect();
    ns.sort_unstable();
    ns.iter().rev().find(|n| dir.join(format!("_main.{n}.ok")).exists()).or(ns.last()).copied()
}

/// Files of a folder world that belong to its current save: every chunk file and other file, but of
/// the `_main.<N>.*` sets only the one that loads (a save in progress or a leftover isn't copied).
pub fn save_files(dir: &Path) -> Vec<PathBuf> {
    let keep = latest_main(dir).map(|n| format!("_main.{n}."));
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            !n.starts_with("_main.") || keep.as_deref().map(|k| n.starts_with(k)).unwrap_or(true)
        })
        .map(|e| e.path())
        .collect()
}

/// A world folder (`_main.N.fwl2` inside) – None when it isn't one.
pub fn read_folder_world(dir: &Path) -> Option<World> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    if name.starts_with('_') || name.starts_with('.') || name.contains("_backup_") {
        return None;
    }
    let n = latest_main(dir)?;
    let fwl = dir.join(format!("_main.{n}.fwl2"));
    let db = dir.join(format!("_main.{n}.db2"));
    let meta = std::fs::read(&fwl).ok().and_then(|b| parse_meta(&b));
    // A server that fails to start (bad password – verified) leaves a `DevWorld` stub: metadata
    // without a seed and no world data. Not a world anyone made.
    if !db.exists() && meta.as_ref().map(|m| m.seed_name.is_empty()).unwrap_or(true) {
        return None;
    }
    Some(World {
        name,
        format: "folder".into(),
        active: false,
        save_number: Some(n),
        day: read_world_time(&db).map(day_of),
        saved_at: Some(mtime(&fwl)).filter(|t| *t > 0),
        size: dir_size(dir),
        path: dir.to_path_buf(),
        backups: vec![],
        has_old: false,
        meta,
    })
}

/// A legacy `<name>.fwl` (+ `.db`).
pub fn read_legacy_world(fwl: &Path) -> Option<World> {
    let stem = fwl.file_stem()?.to_string_lossy().into_owned();
    if fwl.extension()?.to_string_lossy() != "fwl" || stem.contains("_backup_") || stem.starts_with('.') {
        return None;
    }
    let db = fwl.with_extension("db");
    let dir = fwl.parent()?;
    Some(World {
        format: "legacy".into(),
        active: false,
        save_number: None,
        meta: std::fs::read(fwl).ok().and_then(|b| parse_meta(&b)),
        day: read_world_time(&db).map(day_of),
        saved_at: Some(mtime(&db).max(mtime(fwl))).filter(|t| *t > 0),
        size: [fwl.to_path_buf(), db.clone()].iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum(),
        path: fwl.to_path_buf(),
        backups: vec![],
        has_old: dir.join(format!("{stem}.db.old")).exists(),
        name: stem,
    })
}

fn backup_kind_time(name: &str, path: &Path) -> (String, i64) {
    let c = BACKUP_RE.captures(name);
    let kind = c.as_ref().and_then(|c| c.get(2)).map(|m| m.as_str().to_string()).unwrap_or_else(|| "manual".into());
    let t = c
        .as_ref()
        .and_then(|c| c.get(3))
        .and_then(|m| {
            let s = m.as_str().replace('-', "");
            chrono::NaiveDateTime::parse_from_str(&s, "%Y%m%d%H%M%S").ok()
        })
        .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|| mtime(path));
    (kind, t)
}

/// Every world in a `worlds_local`-style folder with its backups, the active one first.
pub fn scan_dir(root: &Path, active: Option<&str>) -> Vec<World> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    let entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    let mut worlds: Vec<World> = vec![];
    for p in &entries {
        let w = if p.is_dir() { read_folder_world(p) } else { read_legacy_world(p) };
        if let Some(w) = w {
            // A converted world can have both layouts – the folder wins.
            if let Some(existing) = worlds.iter().position(|x| x.name.eq_ignore_ascii_case(&w.name)) {
                if worlds[existing].format == "legacy" && w.format == "folder" {
                    worlds[existing] = w;
                }
                continue;
            }
            worlds.push(w);
        }
    }
    for w in worlds.iter_mut() {
        let prefix = format!("{}_backup_", w.name).to_lowercase();
        let mut seen: Vec<String> = vec![];
        for p in &entries {
            let file = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !file.to_lowercase().starts_with(&prefix) {
                continue;
            }
            // Legacy backups are a .fwl + .db pair – list them once.
            let stem = if p.is_dir() { file.clone() } else { p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() };
            if seen.contains(&stem) || (!p.is_dir() && !matches!(p.extension().and_then(|e| e.to_str()), Some("fwl") | Some("db"))) {
                continue;
            }
            seen.push(stem.clone());
            let (kind, created_at) = backup_kind_time(&stem, p);
            let size = if p.is_dir() { dir_size(p) } else { ["fwl", "db"].iter().filter_map(|x| std::fs::metadata(root.join(format!("{stem}.{x}"))).ok()).map(|m| m.len()).sum() };
            w.backups.push(WorldBackup { name: stem, kind, created_at, size });
        }
        w.backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        w.active = active.map(|a| a.eq_ignore_ascii_case(&w.name)).unwrap_or(false);
    }
    worlds.sort_by(|a, b| b.active.cmp(&a.active).then(b.saved_at.cmp(&a.saved_at)));
    worlds
}

pub fn list(inst: &ServerInstance) -> Vec<World> {
    scan_dir(&inst.worlds_dir(), Some(inst.config.world.trim()))
}

/// The in-game day of the server's world at its last save.
pub fn active_day(inst: &ServerInstance) -> Option<u32> {
    let dir = inst.world_dir();
    if let Some(n) = latest_main(&dir) {
        return read_world_time(&dir.join(format!("_main.{n}.db2"))).map(day_of);
    }
    read_world_time(&inst.worlds_dir().join(format!("{}.db", inst.config.world.trim()))).map(day_of)
}

/// Worlds in any folder the user picks: a `worlds_local` / `worlds` folder, the save folder
/// above it, or one world folder.
pub fn scan_any(dir: &Path) -> Vec<World> {
    if let Some(w) = read_folder_world(dir) {
        return vec![w];
    }
    let direct = scan_dir(dir, None);
    if !direct.is_empty() {
        return direct;
    }
    let mut out = scan_dir(&dir.join(WORLDS_DIR), None);
    out.extend(scan_dir(&dir.join("worlds"), None));
    out
}

/// A world name that's also a safe folder name.
pub fn check_name(name: &str) -> AppResult<&str> {
    let n = name.trim();
    if n.is_empty() || n.len() > 64 || n.starts_with('.') || n.starts_with('_') || n.contains("_backup_") || n.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(AppError::msg(format!("“{n}” can't be used as a world name – it becomes a folder name.")));
    }
    Ok(n)
}

fn find(inst: &ServerInstance, name: &str) -> AppResult<World> {
    let name = check_name(name)?;
    list(inst).into_iter().find(|w| w.name.eq_ignore_ascii_case(name)).ok_or_else(|| AppError::msg(format!("World “{name}” wasn't found.")))
}

/// Every path that makes up a world (the folder, or the legacy files).
fn world_paths(w: &World) -> Vec<PathBuf> {
    if w.format == "folder" {
        return vec![w.path.clone()];
    }
    let dir = w.path.parent().unwrap_or(Path::new("."));
    ["fwl", "db", "fwl.old", "db.old"].iter().map(|x| dir.join(format!("{}.{x}", w.name))).filter(|p| p.exists()).collect()
}

fn name_taken(inst: &ServerInstance, name: &str) -> bool {
    let d = inst.worlds_dir();
    d.join(name).exists() || d.join(format!("{name}.fwl")).exists()
}

/// A free world name based on `base` (`base`, `base-2` …).
pub fn unique_name(inst: &ServerInstance, base: &str) -> String {
    let base = base.trim();
    let mut n = base.to_string();
    let mut i = 2;
    while name_taken(inst, &n) {
        n = format!("{base}-{i}");
        i += 1;
    }
    n
}

/// Move a file or folder into `<savedir>\_quarantine\<stamp>-<label>\` (recoverable). Returns the folder.
pub fn quarantine(inst: &ServerInstance, paths: &[PathBuf], label: &str) -> AppResult<Option<PathBuf>> {
    let paths: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
    if paths.is_empty() {
        return Ok(None);
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let q = inst.data_dir().join("_quarantine").join(format!("{stamp}-{label}"));
    std::fs::create_dir_all(&q)?;
    for p in paths {
        let target = q.join(p.file_name().unwrap_or_default());
        if std::fs::rename(p, &target).is_err() {
            // Something holds a handle on the folder itself (an Explorer window, a sync tool, an
            // antivirus scan): moving what's inside still works.
            if p.is_dir() {
                move_contents(p, &target)?;
                let _ = std::fs::remove_dir(p);
            } else {
                std::fs::copy(p, &target)?;
                std::fs::remove_file(p)?;
            }
        }
    }
    Ok(Some(q))
}

fn move_contents(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let target = to.join(e.file_name());
        if std::fs::rename(e.path(), &target).is_err() {
            if e.path().is_dir() {
                move_contents(&e.path(), &target)?;
                let _ = std::fs::remove_dir(e.path());
            } else {
                std::fs::copy(e.path(), &target)?;
                std::fs::remove_file(e.path())?;
            }
        }
    }
    Ok(())
}

/// Rewrite the name stored in every world-metadata file of a copied world.
fn set_world_name(paths: &[PathBuf], new_name: &str) -> AppResult<()> {
    let mut files = vec![];
    for p in paths {
        if p.is_dir() {
            for e in std::fs::read_dir(p)?.flatten() {
                if MAIN_RE.is_match(&e.file_name().to_string_lossy()) {
                    files.push(e.path());
                }
            }
        } else if p.extension().map(|x| x == "fwl").unwrap_or(false) || p.to_string_lossy().ends_with(".fwl.old") {
            files.push(p.clone());
        }
    }
    for f in files {
        let bytes = std::fs::read(&f)?;
        let out = rename_meta(&bytes, new_name).ok_or_else(|| AppError::msg(format!("{} isn't a world file the manager can read.", f.display())))?;
        std::fs::write(&f, out)?;
    }
    Ok(())
}

/// Copy a world (folder or legacy pair) to `dest_dir` as `new_name`, rewriting its internal name.
fn copy_world(w: &World, dest_dir: &Path, new_name: &str) -> AppResult<usize> {
    std::fs::create_dir_all(dest_dir)?;
    let mut written = vec![];
    let mut n = 0;
    if w.format == "folder" {
        // The loading save and its chunks only – safe while the server is writing the next save.
        let to = dest_dir.join(new_name);
        std::fs::create_dir_all(&to)?;
        for f in save_files(&w.path) {
            std::fs::copy(&f, to.join(f.file_name().unwrap_or_default()))?;
            n += 1;
        }
        written.push(to);
    } else {
        for p in world_paths(w) {
            let file = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let to = dest_dir.join(format!("{new_name}{}", &file[w.name.len()..]));
            std::fs::copy(&p, &to)?;
            written.push(to);
            n += 1;
        }
    }
    if new_name != w.name {
        set_world_name(&written, new_name)?;
    }
    Ok(n)
}

/// Copy a world in from another folder (the game's saves, an old server) under a free name.
pub fn import(inst: &ServerInstance, src: &Path, name: &str) -> AppResult<(String, usize)> {
    let name = check_name(name)?;
    let w = if src.is_dir() { read_folder_world(src) } else { read_legacy_world(src) }.ok_or_else(|| AppError::msg("That isn't a Valheim world (a world folder with _main.N.fwl2, or a .fwl file)."))?;
    if super::proc::is_within(&w.path, &inst.worlds_dir()) {
        return Err(AppError::msg("That world is already on this server."));
    }
    let target = unique_name(inst, name);
    let n = copy_world(&w, &inst.worlds_dir(), &target)?;
    Ok((target, n))
}

/// Copy a world on this server under a new name (to branch off a season, test a change …).
pub fn duplicate(inst: &ServerInstance, name: &str, new_name: &str) -> AppResult<String> {
    let w = find(inst, name)?;
    let target = unique_name(inst, check_name(new_name)?);
    copy_world(&w, &inst.worlds_dir(), &target)?;
    Ok(target)
}

/// Rename a world (folder/files and the name inside them). The caller moves `-world` along.
pub fn rename(inst: &ServerInstance, name: &str, new_name: &str) -> AppResult<String> {
    let w = find(inst, name)?;
    let new_name = check_name(new_name)?.to_string();
    if new_name == w.name {
        return Ok(new_name);
    }
    if !new_name.eq_ignore_ascii_case(&w.name) && name_taken(inst, &new_name) {
        return Err(AppError::msg(format!("A world called “{new_name}” already exists.")));
    }
    // Copy under the new name first, then retire the old copy – never a half-renamed world.
    let staged = inst.worlds_dir().join("_staging");
    let _ = std::fs::remove_dir_all(&staged);
    copy_world(&w, &staged, &new_name).map_err(|e| AppError::msg(format!("Couldn't copy the world ({e}).")))?;
    quarantine(inst, &world_paths(&w), "renamed")?;
    for e in std::fs::read_dir(&staged)?.flatten() {
        std::fs::rename(e.path(), inst.worlds_dir().join(e.file_name()))?;
    }
    let _ = std::fs::remove_dir_all(&staged);
    Ok(new_name)
}

/// Replace a world with one of the server's own backups (the current world is quarantined first).
pub fn restore_backup(inst: &ServerInstance, name: &str, backup: &str) -> AppResult<String> {
    let w = find(inst, name)?;
    let b = w.backups.iter().find(|b| b.name == backup).ok_or_else(|| AppError::msg("That backup doesn't exist any more."))?;
    let root = inst.worlds_dir();
    let src_dir = root.join(&b.name);
    let bw = if src_dir.is_dir() {
        // A backup folder is a whole world folder under another name.
        let mut x = read_folder_world(&PathBuf::from(&src_dir)).or_else(|| {
            let n = latest_main(&src_dir)?;
            Some(World { name: b.name.clone(), format: "folder".into(), active: false, save_number: Some(n), meta: None, day: None, saved_at: None, size: 0, path: src_dir.clone(), backups: vec![], has_old: false })
        });
        if let Some(x) = x.as_mut() {
            x.name = b.name.clone();
        }
        x
    } else {
        read_legacy_world(&root.join(format!("{}.fwl", b.name))).map(|mut x| {
            x.name = b.name.clone();
            x
        })
    }
    .ok_or_else(|| AppError::msg("That backup can't be read."))?;
    quarantine(inst, &world_paths(&w), "before-rollback")?;
    copy_world(&bw, &root, &w.name)?;
    Ok(b.name.clone())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCheck {
    /// "Current save" or the backup's name.
    pub file: String,
    pub ok: bool,
    /// Uncompressed size of the world data (what the server loads).
    pub bytes: u64,
    pub error: Option<String>,
}

/// Unpack the world data in a `.db2`: int32 version, double world time, int32 length, then a gzip
/// block of that length (verified: 145 772 bytes of gzip at offset 16, followed by 40 more bytes of
/// other data). Decoding the block to its end validates its CRC. `None` when there's no gzip block.
pub fn unpack_db2(bytes: &[u8]) -> Option<Result<u64, String>> {
    let pos = bytes.windows(2).take(64).position(|w| w == [0x1f, 0x8b])?;
    let len = (pos >= 4).then(|| i32::from_le_bytes([bytes[pos - 4], bytes[pos - 3], bytes[pos - 2], bytes[pos - 1]])).filter(|n| *n > 0).map(|n| n as usize);
    let block = match len {
        Some(n) if pos + n <= bytes.len() => &bytes[pos..pos + n],
        Some(_) => return Some(Err("cut off (the save is shorter than its header says)".into())),
        None => &bytes[pos..],
    };
    let mut d = flate2::read::GzDecoder::new(block);
    Some(std::io::copy(&mut d, &mut std::io::sink()).map_err(|e| format!("damaged ({e})")))
}

fn check_db(path: &Path) -> Result<u64, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("missing ({e})"))?;
    if bytes.len() < 16 {
        return Err("empty".into());
    }
    match unpack_db2(&bytes) {
        Some(r) => r,
        // Legacy .db files aren't compressed – a readable header is all there is to check.
        None => read_world_time(path).map(|_| bytes.len() as u64).ok_or_else(|| "unreadable header".into()),
    }
}

fn check_world_files(label: &str, w: &World) -> SaveCheck {
    let mut c = SaveCheck { file: label.into(), ok: true, bytes: 0, error: None };
    let res: Result<u64, String> = if w.format == "folder" {
        match w.save_number {
            Some(n) if !w.path.join(format!("_main.{n}.ok")).exists() => Err(format!("save {n} was never finished (no _main.{n}.ok)")),
            Some(n) => check_db(&w.path.join(format!("_main.{n}.db2"))),
            None => Err("no save in the folder".into()),
        }
    } else {
        check_db(&w.path.with_extension("db"))
    };
    match res {
        Ok(b) => c.bytes = b,
        Err(e) => {
            c.ok = false;
            c.error = Some(e);
        }
    }
    if c.ok && w.meta.is_none() {
        c.ok = false;
        c.error = Some("the world metadata (.fwl2) can't be read".into());
    }
    c
}

/// Check the current save of a world and every backup of it.
pub fn check(inst: &ServerInstance, name: &str) -> AppResult<Vec<SaveCheck>> {
    let w = find(inst, name)?;
    let root = inst.worlds_dir();
    let mut out = vec![check_world_files("Current save", &w)];
    for b in &w.backups {
        let p = root.join(&b.name);
        let bw = if p.is_dir() {
            latest_main(&p).map(|n| World {
                name: b.name.clone(),
                format: "folder".into(),
                active: false,
                save_number: Some(n),
                meta: std::fs::read(p.join(format!("_main.{n}.fwl2"))).ok().and_then(|x| parse_meta(&x)),
                day: None,
                saved_at: None,
                size: 0,
                path: p.clone(),
                backups: vec![],
                has_old: false,
            })
        } else {
            read_legacy_world(&root.join(format!("{}.fwl", b.name)))
        };
        out.push(match bw {
            Some(bw) => check_world_files(&b.name, &bw),
            None => SaveCheck { file: b.name.clone(), ok: false, bytes: 0, error: Some("unreadable".into()) },
        });
    }
    Ok(out)
}

pub fn remove(inst: &ServerInstance, name: &str) -> AppResult<Option<PathBuf>> {
    let w = find(inst, name)?;
    quarantine(inst, &world_paths(&w), "deleted")
}

/// Quarantine the active world so the next start generates a brand-new one under the same name.
pub fn reset(inst: &ServerInstance) -> AppResult<Option<PathBuf>> {
    match find(inst, &inst.config.world) {
        Ok(w) => quarantine(inst, &world_paths(&w), "world-reset"),
        Err(_) => Ok(None),
    }
}

/// Copy a world out (to keep, share or load in the game / another server).
pub fn export(inst: &ServerInstance, name: &str, dest: &Path) -> AppResult<usize> {
    let w = find(inst, name)?;
    if dest.join(&w.name).exists() || dest.join(format!("{}.fwl", w.name)).exists() {
        return Err(AppError::msg(format!("{} already has a world called “{}”.", dest.display(), w.name)));
    }
    copy_world(&w, dest, &w.name)
}

// ---------------------------------------------------------------------------------------------
// Worlds of the Valheim game on this PC.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorld {
    pub world: World,
    /// "Local worlds" | "Steam Cloud (<account>)"
    pub source: String,
}

/// Worlds in `%USERPROFILE%\AppData\LocalLow\IronGate\Valheim\worlds_local` (the server's default
/// save path too, per the manual) and Steam Cloud copies in `Steam\userdata\<id>\892970\remote\worlds`.
pub fn local_worlds() -> Vec<LocalWorld> {
    let mut out = vec![];
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let base = home.join("AppData").join("LocalLow").join("IronGate").join("Valheim");
        for w in scan_dir(&base.join(WORLDS_DIR), None) {
            out.push(LocalWorld { world: w, source: "Local worlds".into() });
        }
        for w in scan_dir(&base.join("worlds"), None) {
            out.push(LocalWorld { world: w, source: "Cloud cache".into() });
        }
    }
    for steam in super::steamcmd::steam_roots() {
        let Ok(users) = std::fs::read_dir(steam.join("userdata")) else { continue };
        for u in users.flatten() {
            for w in scan_dir(&u.path().join(GAME_APP_ID.to_string()).join("remote").join("worlds"), None) {
                out.push(LocalWorld { world: w, source: format!("Steam Cloud ({})", u.file_name().to_string_lossy()) });
            }
        }
    }
    out.sort_by(|a, b| b.world.saved_at.cmp(&a.world.saved_at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `_main.6.fwl2` from a real 1.0.20 world created with `-preset hard -modifier raids none -setkey nomap`.
    fn real_fwl2() -> Vec<u8> {
        let mut payload = 42i32.to_le_bytes().to_vec();
        write_string(&mut payload, "Dedicated");
        write_string(&mut payload, "Jk5aSQlv4V");
        payload.extend(0x28E1B767i32.to_le_bytes());
        payload.extend(0xA15CB44Fu64.to_le_bytes());
        payload.extend(2i32.to_le_bytes());
        payload.push(1);
        let keys = ["playerdamage 85", "enemydamage 150", "preset combat_hard:deathpenalty_default:resources_default:raids_none:portals_default", "nomap"];
        payload.extend((keys.len() as i32).to_le_bytes());
        for k in keys {
            write_string(&mut payload, k);
        }
        payload.extend([0, 0, 0]);
        let mut out = (payload.len() as i32).to_le_bytes().to_vec();
        out.extend(payload);
        out
    }

    #[test]
    fn parses_and_renames_world_metadata() {
        // Header bytes as they appear in the real file.
        let b = real_fwl2();
        assert_eq!(&b[4..18], &[0x2A, 0, 0, 0, 9, b'D', b'e', b'd', b'i', b'c', b'a', b't', b'e', b'd']);
        let m = parse_meta(&b).unwrap();
        assert_eq!((m.name.as_str(), m.seed_name.as_str(), m.version), ("Dedicated", "Jk5aSQlv4V", 42));
        assert_eq!(m.keys.len(), 4);
        assert_eq!(m.keys[3], "nomap");
        let r = rename_meta(&b, "Midgård Season 2").unwrap();
        let m2 = parse_meta(&r).unwrap();
        assert_eq!(m2.name, "Midgård Season 2");
        assert_eq!((m2.seed_name, m2.keys), (m.seed_name, m.keys));
        assert_eq!(i32::from_le_bytes([r[0], r[1], r[2], r[3]]) as usize, r.len() - 4);
        assert_eq!(day_of(2040.0), 1);
        assert_eq!(day_of(1799.0), 0);
    }

    fn inst() -> ServerInstance {
        let d = std::env::temp_dir().join(format!("vhsm-worlds-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        ServerInstance { install_dir: d, ..Default::default() }
    }

    fn db2(time: f64, ok: bool) -> Vec<u8> {
        use std::io::Write;
        let mut v = 42i32.to_le_bytes().to_vec();
        v.extend(time.to_le_bytes());
        let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        gz.write_all(&[9u8; 3000]).unwrap();
        let g = gz.finish().unwrap();
        v.extend((g.len() as i32).to_le_bytes());
        v.extend(if ok { g.clone() } else { g[..g.len() / 2].to_vec() });
        // Real saves carry more data after the gzip block (40 bytes in a fresh world).
        if ok {
            v.extend([0xbc, 0x6a, 0xcc, 0x43]);
            v.extend([0u8; 36]);
        }
        v
    }

    fn make_world(dir: &Path, n: u32, time: f64, ok: bool) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("_main.{n}.fwl2")), real_fwl2()).unwrap();
        std::fs::write(dir.join(format!("_main.{n}.db2")), db2(time, ok)).unwrap();
        std::fs::write(dir.join(format!("_main.{n}.ok")), 42i32.to_le_bytes()).unwrap();
        std::fs::write(dir.join("00_00__0_1.chunk"), b"chunk").unwrap();
    }

    #[test]
    fn lists_checks_and_rolls_back() {
        let i = inst();
        let wl = i.worlds_dir();
        make_world(&wl.join("Dedicated"), 6, 2040.0 + 1800.0 * 4.0, true);
        make_world(&wl.join("Dedicated_backup_auto-20261016-081500"), 3, 2040.0, true);
        make_world(&wl.join("Other"), 1, 100.0, false);
        std::fs::write(wl.join("Old.fwl"), real_fwl2()).unwrap();
        std::fs::write(wl.join("Old.db"), db2(5000.0, true)).unwrap();
        let ws = list(&i);
        assert_eq!(ws.len(), 3, "{ws:?}");
        assert!(ws[0].active && ws[0].name == "Dedicated");
        assert_eq!((ws[0].save_number, ws[0].day), (Some(6), Some(5)));
        assert_eq!(ws[0].backups.len(), 1);
        assert_eq!(ws[0].backups[0].kind, "auto");
        assert_eq!(active_day(&i), Some(5));
        let legacy = ws.iter().find(|w| w.name == "Old").unwrap();
        assert_eq!((legacy.format.as_str(), legacy.day), ("legacy", Some(2)));
        let c = check(&i, "Other").unwrap();
        assert!(!c[0].ok, "a cut-off db2 is reported");
        let c = check(&i, "Dedicated").unwrap();
        assert!(c.iter().all(|x| x.ok), "{c:?}");
        // Roll back: the backup's world becomes "Dedicated" and the current one is kept aside.
        restore_backup(&i, "Dedicated", "Dedicated_backup_auto-20261016-081500").unwrap();
        let w = list(&i).into_iter().find(|w| w.name == "Dedicated").unwrap();
        assert_eq!(w.save_number, Some(3));
        assert_eq!(w.meta.unwrap().name, "Dedicated");
        assert!(i.data_dir().join("_quarantine").read_dir().unwrap().next().is_some());
        let _ = std::fs::remove_dir_all(&i.install_dir);
    }

    #[test]
    fn a_save_in_progress_is_never_picked_or_copied() {
        let i = inst();
        let w = i.worlds_dir().join("Dedicated");
        make_world(&w, 6, 2040.0, true);
        // The server is writing save 7: data is there, the .ok marker isn't yet.
        std::fs::write(w.join("_main.7.fwl2"), real_fwl2()).unwrap();
        std::fs::write(w.join("_main.7.db2"), b"half").unwrap();
        assert_eq!(latest_main(&w), Some(6));
        let files: Vec<String> = save_files(&w).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert!(files.iter().all(|f| !f.starts_with("_main.7")), "{files:?}");
        assert!(files.contains(&"_main.6.ok".to_string()) && files.contains(&"00_00__0_1.chunk".to_string()));
        assert_eq!(duplicate(&i, "Dedicated", "Copy").unwrap(), "Copy");
        assert!(!i.worlds_dir().join("Copy").join("_main.7.db2").exists());
        let _ = std::fs::remove_dir_all(&i.install_dir);
    }

    #[test]
    fn rename_duplicate_import_rewrite_the_inner_name() {
        let i = inst();
        let wl = i.worlds_dir();
        make_world(&wl.join("Dedicated"), 2, 2040.0, true);
        assert!(rename(&i, "Dedicated", "bad/name").is_err());
        assert_eq!(rename(&i, "Dedicated", "Season 2").unwrap(), "Season 2");
        let w = list(&i);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].meta.as_ref().unwrap().name, "Season 2");
        assert_eq!(duplicate(&i, "Season 2", "Copy").unwrap(), "Copy");
        assert_eq!(list(&i).iter().find(|w| w.name == "Copy").unwrap().meta.as_ref().unwrap().name, "Copy");
        // Import a legacy world from "the game".
        let game = i.install_dir.join("game").join("worlds_local");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Home.fwl"), real_fwl2()).unwrap();
        std::fs::write(game.join("Home.db"), db2(9000.0, true)).unwrap();
        assert_eq!(scan_any(game.parent().unwrap()).len(), 1);
        let (name, n) = import(&i, &game.join("Home.fwl"), "Copy").unwrap();
        assert_eq!((name.as_str(), n), ("Copy-2", 2));
        assert_eq!(parse_meta(&std::fs::read(wl.join("Copy-2.fwl")).unwrap()).unwrap().name, "Copy-2");
        assert!(remove(&i, "Copy").unwrap().is_some());
        assert!(!wl.join("Copy").exists());
        let _ = std::fs::remove_dir_all(&i.install_dir);
    }
}
