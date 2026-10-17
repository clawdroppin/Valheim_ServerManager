//! Zip backups, restore, retention and portable server packs (`.valheim`).
//!
//! A backup is a point-in-time snapshot of every world in `worlds_local` (each world folder, or a
//! legacy `.fwl` + `.db` pair – `worlds_local/…` in the zip; the server's own `_backup_` copies
//! are left out) and the three permission lists (`Lists/…`).

use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::state::{now_ms, AppState};
use std::io::{Read, Write};
use std::path::Path;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

const MANIFEST: &str = "backup.json";
const LISTS: &str = "Lists";

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(6))
}

fn add_file<W: Write + std::io::Seek>(zip: &mut zip::ZipWriter<W>, path: &Path, name: &str) -> AppResult<u32> {
    let Ok(buf) = std::fs::read(path) else { return Ok(0) };
    zip.start_file(name, opts())?;
    zip.write_all(&buf)?;
    Ok(1)
}

/// `Lists/` – adminlist.txt, bannedlist.txt, permittedlist.txt.
fn add_lists<W: Write + std::io::Seek>(zip: &mut zip::ZipWriter<W>, inst: &ServerInstance) -> AppResult<u32> {
    let mut n = 0;
    for f in [ADMIN_FILE, BAN_FILE, PERMIT_FILE] {
        n += add_file(zip, &inst.data_dir().join(f), &format!("{LISTS}/{f}"))?;
    }
    Ok(n)
}

/// `worlds_local/…`: every world (not the server's own backups of them).
fn add_worlds<W: Write + std::io::Seek>(zip: &mut zip::ZipWriter<W>, inst: &ServerInstance) -> AppResult<u32> {
    let mut n = 0;
    for w in super::worlds::list(inst) {
        if w.format == "folder" {
            // Only the save that loads – a save the server is writing right now isn't half-copied.
            for f in super::worlds::save_files(&w.path) {
                let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
                n += add_file(zip, &f, &format!("{WORLDS_DIR}/{}/{name}", w.name))?;
            }
        } else {
            for x in ["fwl", "db", "fwl.old", "db.old"] {
                let p = inst.worlds_dir().join(format!("{}.{x}", w.name));
                if p.exists() {
                    n += add_file(zip, &p, &format!("{WORLDS_DIR}/{}.{x}", w.name))?;
                }
            }
        }
    }
    Ok(n)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    label: String,
    kind: String,
    created_at: i64,
    server_name: String,
    files: u32,
}

pub async fn create(state: &AppState, inst: &ServerInstance, label: &str, kind: &str) -> AppResult<BackupInfo> {
    let dir = state.backup_dir(inst).await;
    let inst = inst.clone();
    let label = label.to_string();
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        let info = create_blocking(&inst, &dir, &label, &kind)?;
        prune(&dir, inst.automation.backup_keep);
        Ok(info)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

fn create_blocking(inst: &ServerInstance, dir: &Path, label: &str, kind: &str) -> AppResult<BackupInfo> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let path = dir.join(format!("{stamp}_{kind}.zip"));
    let tmp = path.with_extension("zip.part");
    let file = std::fs::File::create(&tmp)?;
    let mut zip = zip::ZipWriter::new(file);
    let mut files = add_worlds(&mut zip, inst)?;
    files += add_lists(&mut zip, inst)?;
    let created_at = now_ms();
    let m = Manifest { label: label.to_string(), kind: kind.to_string(), created_at, server_name: inst.name.clone(), files };
    zip.start_file(MANIFEST, opts())?;
    zip.write_all(&serde_json::to_vec_pretty(&m)?)?;
    zip.finish()?;
    std::fs::rename(&tmp, &path)?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    Ok(BackupInfo {
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        path,
        size,
        created_at,
        label: label.to_string(),
        kind: kind.to_string(),
        files,
    })
}

fn read_manifest(path: &Path) -> Option<Manifest> {
    let f = std::fs::File::open(path).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    let mut e = z.by_name(MANIFEST).ok()?;
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn list(dir: &Path) -> Vec<BackupInfo> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<BackupInfo> = rd
        .flatten()
        .filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false))
        .map(|e| {
            let path = e.path();
            let meta = e.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta.and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()).unwrap_or(0);
            let m = read_manifest(&path);
            BackupInfo {
                name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                size,
                created_at: m.as_ref().map(|m| m.created_at).unwrap_or(mtime),
                label: m.as_ref().map(|m| m.label.clone()).unwrap_or_else(|| "Imported".into()),
                kind: m.as_ref().map(|m| m.kind.clone()).unwrap_or_else(|| "manual".into()),
                files: m.as_ref().map(|m| m.files).unwrap_or(0),
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

/// Keep the newest `keep` automatic backups; manual ones are never pruned.
pub fn prune(dir: &Path, keep: u32) -> u32 {
    if keep == 0 {
        return 0;
    }
    let autos: Vec<BackupInfo> = list(dir).into_iter().filter(|b| b.kind != "manual").collect();
    let mut removed = 0;
    for b in autos.iter().skip(keep as usize) {
        if std::fs::remove_file(&b.path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn extract_prefix(zip_path: &Path, prefix: &str, dest: &Path) -> AppResult<u32> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let Some(name) = e.enclosed_name() else { continue };
        let name_s = name.to_string_lossy().replace('\\', "/");
        let Some(rel) = name_s.strip_prefix(&format!("{prefix}/")) else { continue };
        if rel.is_empty() || e.is_dir() {
            continue;
        }
        let out = dest.join(rel);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut e, &mut w)?;
        n += 1;
    }
    Ok(n)
}

/// Names of the worlds contained in a backup or pack.
fn zip_worlds(zip_path: &Path) -> Vec<String> {
    let Ok(f) = std::fs::File::open(zip_path) else { return vec![] };
    let Ok(z) = zip::ZipArchive::new(f) else { return vec![] };
    let mut out: Vec<String> = vec![];
    for n in z.file_names().flatten() {
        let parts: Vec<&str> = n.split('/').collect();
        if parts.len() < 2 || parts[0] != WORLDS_DIR || parts[1].is_empty() {
            continue;
        }
        let w = if parts.len() == 2 { parts[1].split('.').next().unwrap_or("").to_string() } else { parts[1].to_string() };
        if !w.is_empty() && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Restore a backup into a stopped server. Each world in the backup replaces the world of the same
/// name (which is quarantined first); other worlds stay. The lists (when asked for) go through the
/// config history like any other edit.
pub fn restore(inst: &ServerInstance, zip_path: &Path, include_lists: bool) -> AppResult<u32> {
    let root = inst.worlds_dir();
    for w in zip_worlds(zip_path) {
        let paths: Vec<std::path::PathBuf> = std::iter::once(root.join(&w)).chain(["fwl", "db", "fwl.old", "db.old"].iter().map(|x| root.join(format!("{w}.{x}")))).collect();
        super::worlds::quarantine(inst, &paths, "before-restore")?;
    }
    std::fs::create_dir_all(&root)?;
    let mut n = extract_prefix(zip_path, WORLDS_DIR, &root)?;
    if include_lists {
        n += restore_lists(zip_path, inst)?;
    }
    Ok(n)
}

/// Write the `Lists/` files of a zip into the server's save folder.
fn restore_lists(zip_path: &Path, inst: &ServerInstance) -> AppResult<u32> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    let dir = inst.data_dir();
    std::fs::create_dir_all(&dir)?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        let name = e.name().map(|c| c.into_owned()).unwrap_or_default();
        let Some(file) = name.strip_prefix(&format!("{LISTS}/")) else { continue };
        if ![ADMIN_FILE, BAN_FILE, PERMIT_FILE].contains(&file) || e.is_dir() {
            continue;
        }
        let mut s = String::new();
        e.read_to_string(&mut s)?;
        super::config::write_text(&dir.join(file), &s)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------------------------
// Server packs (.valheim): a zip with manifest.json + Lists/ + optional worlds_local/.

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackOptions {
    pub include_saves: bool,
    pub include_secrets: bool,
}

pub fn export_pack(inst: &ServerInstance, dest: &Path, o: &PackOptions) -> AppResult<()> {
    let mut profile = inst.clone();
    if !o.include_secrets {
        profile.config.password.clear();
        profile.branch_password.clear();
    }
    let tmp = dest.with_extension("part");
    let file = std::fs::File::create(&tmp)?;
    let mut zip = zip::ZipWriter::new(file);
    let lists = add_lists(&mut zip, inst)?;
    let has_saves = if o.include_saves { add_worlds(&mut zip, inst)? > 0 } else { false };
    let m = PackManifest {
        format: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        exported_at: now_ms(),
        instance: profile,
        has_saves,
        has_config: lists > 0,
        includes_secrets: o.include_secrets,
    };
    zip.start_file("manifest.json", opts())?;
    zip.write_all(&serde_json::to_vec_pretty(&m)?)?;
    zip.finish()?;
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

pub fn read_pack(path: &Path) -> AppResult<PackManifest> {
    let f = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(f).map_err(|_| AppError::msg("That file isn't a Valheim server pack."))?;
    let mut e = z.by_name("manifest.json").map_err(|_| AppError::msg("The pack has no manifest.json – not a server pack."))?;
    let mut s = String::new();
    e.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// Unpack the lists (and worlds) of a pack into a (new) server's folders.
pub fn unpack_into(path: &Path, inst: &ServerInstance) -> AppResult<()> {
    std::fs::create_dir_all(&inst.install_dir)?;
    restore_lists(path, inst)?;
    if !zip_worlds(path).is_empty() {
        let root = inst.worlds_dir();
        std::fs::create_dir_all(&root)?;
        extract_prefix(path, WORLDS_DIR, &root)?;
    }
    Ok(())
}

/// Read every entry of a backup (the zip checks each CRC) and unpack the world data inside –
/// proves it can be restored and that the worlds in it load.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub ok: bool,
    pub files: u32,
    pub worlds: Vec<String>,
    pub has_settings: bool,
    pub bytes: u64,
    pub error: Option<String>,
}

pub fn verify(zip_path: &Path) -> AppResult<VerifyResult> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;
    let mut out = VerifyResult { ok: true, files: 0, worlds: zip_worlds(zip_path), has_settings: false, bytes: 0, error: None };
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        if e.is_dir() {
            continue;
        }
        let name = e.name().map(|n| n.to_string()).unwrap_or_default();
        // Reading to the end makes the zip crate check the entry's CRC32; world data (.db2) is
        // also gunzipped, which catches a save that was already damaged when it was backed up.
        let res = if name.ends_with(".db2") {
            let mut buf = vec![];
            e.read_to_end(&mut buf).and_then(|_| match super::worlds::unpack_db2(&buf) {
                Some(Ok(n)) => Ok(n),
                Some(Err(m)) => Err(std::io::Error::other(m)),
                None => Err(std::io::Error::other("no world data")),
            })
        } else {
            std::io::copy(&mut e, &mut std::io::sink())
        };
        match res {
            Ok(n) => out.bytes += n,
            Err(err) => {
                out.ok = false;
                out.error = Some(format!("{name}: {err}"));
                break;
            }
        }
        out.files += 1;
        if name.starts_with(&format!("{LISTS}/")) {
            out.has_settings = true;
        }
    }
    if out.ok && out.worlds.is_empty() && !out.has_settings {
        out.ok = false;
        out.error = Some("The backup contains neither worlds nor permission lists.".into());
    }
    Ok(out)
}

pub fn folder_size(dir: &Path) -> u64 {
    WalkDir::new(dir).into_iter().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(inst: &ServerInstance, name: &str, n: u32, data: &[u8]) {
        let d = inst.worlds_dir().join(name);
        std::fs::create_dir_all(&d).unwrap();
        let mut fwl = 42i32.to_le_bytes().to_vec();
        fwl.push(name.len() as u8);
        fwl.extend(name.as_bytes());
        fwl.extend([4, b's', b'e', b'e', b'd', 1, 0, 0, 0]);
        let mut f = (fwl.len() as i32).to_le_bytes().to_vec();
        f.extend(fwl);
        std::fs::write(d.join(format!("_main.{n}.fwl2")), f).unwrap();
        std::fs::write(d.join(format!("_main.{n}.db2")), data).unwrap();
        std::fs::write(d.join(format!("_main.{n}.ok")), b"*\0\0\0").unwrap();
    }

    #[test]
    fn backup_restore_round_trip() {
        let root = std::env::temp_dir().join(format!("vhsm-bk-{}", uuid::Uuid::new_v4()));
        let inst = ServerInstance { install_dir: root.join("srv"), ..Default::default() };
        world(&inst, "Dedicated", 1, b"world-v1");
        std::fs::create_dir_all(inst.worlds_dir().join("Dedicated_backup_auto-20261016-081500")).unwrap();
        super::super::config::ensure_lists(&inst).unwrap();
        super::super::config::edit_list(&inst.admin_path(), "76561197960287930", true).unwrap();
        let b = create_blocking(&inst, &root.join("bk"), "test", "manual").unwrap();
        assert_eq!(b.files, 6, "3 world files + 3 lists (no server backups)");
        std::fs::remove_dir_all(inst.world_dir()).unwrap();
        world(&inst, "Dedicated", 2, b"world-v2");
        super::super::config::edit_list(&inst.admin_path(), "76561197960287930", false).unwrap();
        restore(&inst, &b.path, true).unwrap();
        assert_eq!(std::fs::read(inst.world_dir().join("_main.1.db2")).unwrap(), b"world-v1");
        assert!(!inst.world_dir().join("_main.2.db2").exists(), "the world was replaced");
        assert_eq!(super::super::config::read_list(&inst.admin_path()).len(), 1);
        assert!(inst.data_dir().join("_quarantine").exists());
        // The test world data isn't gzip: verify must call that damaged.
        let v = verify(&b.path).unwrap();
        assert!(!v.ok && v.error.as_deref().unwrap_or("").contains("_main.1.db2"), "{v:?}");
        assert_eq!(v.worlds, vec!["Dedicated".to_string()]);
        let listed = list(&root.join("bk"));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "test");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn packs_strip_secrets_and_carry_worlds() {
        let root = std::env::temp_dir().join(format!("vhsm-pack-{}", uuid::Uuid::new_v4()));
        let mut inst = ServerInstance { install_dir: root.join("srv"), ..Default::default() };
        inst.config.password = "join-pw-1".into();
        world(&inst, "Dedicated", 3, b"x");
        super::super::config::ensure_lists(&inst).unwrap();
        let p = root.join("x.valheim");
        export_pack(&inst, &p, &PackOptions { include_saves: true, include_secrets: false }).unwrap();
        let m = read_pack(&p).unwrap();
        assert!(m.instance.config.password.is_empty() && m.has_saves && m.has_config);
        let other = ServerInstance { install_dir: root.join("b"), ..Default::default() };
        unpack_into(&p, &other).unwrap();
        assert!(other.world_dir().join("_main.3.fwl2").exists());
        assert!(other.ban_path().exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
