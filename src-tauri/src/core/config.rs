//! The permission lists (`adminlist.txt`, `bannedlist.txt`, `permittedlist.txt`) and validation of
//! a server profile.
//!
//! Valheim has no settings file: everything the profile owns goes on the command line
//! (`server::build_args`). The three list files live in the `-savedir` folder and are shared with
//! the server – admins edit them in-game (`ban`, `unban`) – so the manager only ever adds or removes
//! single lines and keeps the server's `// List … ONE per line` header and any other comments.
//!
//! Entries are `Platform_UserId` (case sensitive, manual: "follows the format
//! [Platform]_[User ID]"), e.g. `Steam_76561198000000001`. A bare SteamID64 is accepted when
//! reading old files and written as `Steam_<id>`.

use crate::error::{AppError, AppResult};
use crate::models::*;
use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Write with a rolling history (`<folder>\_config-history\<stem>.<stamp>.<ext>`, last 30 per file).
pub fn write_text(path: &Path, text: &str) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        if let Ok(old) = std::fs::read(path) {
            if !old.is_empty() && old != text.as_bytes() {
                let hist = history_dir(path);
                std::fs::create_dir_all(&hist)?;
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ext = path.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "txt".into());
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
                let _ = std::fs::write(hist.join(format!("{stem}.{stamp}.{ext}")), &old);
                prune_history(&hist, &stem, 30);
            }
        }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn history_dir(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).join("_config-history")
}

fn prune_history(dir: &Path, stem: &str, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let prefix = format!("{stem}.");
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with(&prefix)).unwrap_or(false)).collect();
    v.sort();
    let n = v.len();
    for p in v.into_iter().take(n.saturating_sub(keep)) {
        let _ = std::fs::remove_file(p);
    }
}

// ---------------------------------------------------------------------------- permission lists

static PLATFORM_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Steam|Xbox|PlayFab|None)_[A-Za-z0-9\-]{3,64}$").unwrap());

/// A SteamID64: 17 digits starting with 7656.
pub fn is_steam_id(s: &str) -> bool {
    s.len() == 17 && s.starts_with("7656") && s.chars().all(|c| c.is_ascii_digit())
}

/// The canonical list form of a player id: `Steam_<id>` for a bare SteamID64, platform ids as given.
pub fn normalize_id(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches('\u{feff}');
    if is_steam_id(t) {
        return Some(format!("Steam_{t}"));
    }
    if let Some(rest) = t.strip_prefix("steam_").or_else(|| t.strip_prefix("STEAM_")) {
        if is_steam_id(rest) {
            return Some(format!("Steam_{rest}"));
        }
    }
    PLATFORM_ID_RE.is_match(t).then(|| t.to_string())
}

/// The SteamID64 inside a list id (`Steam_7656…` or a bare id).
pub fn steam_part(id: &str) -> Option<String> {
    let t = id.trim();
    let raw = t.strip_prefix("Steam_").unwrap_or(t);
    is_steam_id(raw).then(|| raw.to_string())
}

/// Lines that are entries (not blank, not `//` comments).
fn is_entry(line: &str) -> bool {
    let t = line.trim().trim_start_matches('\u{feff}');
    !t.is_empty() && !t.starts_with("//")
}

/// The ids in a list file, in canonical form (unknown junk is skipped).
pub fn read_list(path: &Path) -> Vec<String> {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut out: Vec<String> = vec![];
    for l in text.lines().filter(|l| is_entry(l)) {
        if let Some(id) = normalize_id(l) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// The header the server writes into a new list file.
pub fn list_header(file: &str) -> &'static str {
    match file {
        ADMIN_FILE => "// List admin players ID  ONE per line",
        BAN_FILE => "// List banned players ID  ONE per line",
        _ => "// List permitted players ID ONE per line",
    }
}

/// Add or remove one id, keeping every comment line. Returns true when the file changed.
pub fn edit_list(path: &Path, id: &str, add: bool) -> AppResult<bool> {
    let Some(id) = normalize_id(id) else {
        return Err(AppError::msg(format!("“{}” isn't a player id – use the SteamID64 (17 digits) or Platform_Id like Steam_7656… / Xbox_….", id.trim())));
    };
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut lines: Vec<String> = if text.is_empty() {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        vec![list_header(&file).to_string()]
    } else {
        text.lines().map(|l| l.to_string()).collect()
    };
    let matches = |l: &str| is_entry(l) && normalize_id(l).as_deref() == Some(id.as_str());
    let had = lines.iter().any(|l| matches(l));
    if add == had {
        return Ok(false);
    }
    if add {
        lines.push(id);
    } else {
        lines.retain(|l| !matches(l));
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut out = lines.join("\n");
    out.push('\n');
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// Create the three list files (with the server's headers) when missing.
pub fn ensure_lists(inst: &ServerInstance) -> AppResult<()> {
    std::fs::create_dir_all(inst.data_dir())?;
    for (f, p) in [(ADMIN_FILE, inst.admin_path()), (BAN_FILE, inst.ban_path()), (PERMIT_FILE, inst.permit_path())] {
        if !p.exists() {
            std::fs::write(&p, format!("{}\n", list_header(f)))?;
        }
    }
    Ok(())
}

/// Lines of a hand-edited list that are neither comments nor valid ids.
pub fn bad_list_lines(text: &str) -> Vec<String> {
    text.lines().filter(|l| is_entry(l) && normalize_id(l).is_none()).map(|l| l.trim().to_string()).take(3).collect()
}

// ---------------------------------------------------------------------------------- validation

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// error = the server would refuse or misbehave; warn = allowed but probably unintended.
    pub level: String,
    pub message: String,
}

fn issue(level: &str, m: impl Into<String>) -> Issue {
    Issue { level: level.into(), message: m.into() }
}

const BAD_NAME_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

pub fn validate(c: &ServerConfig) -> Vec<Issue> {
    let mut out = vec![];
    let name = c.name.trim();
    if name.is_empty() {
        out.push(issue("error", "The server needs a name."));
    } else if name.contains('"') {
        out.push(issue("error", "The server name can't contain double quotes."));
    }
    let world = c.world.trim();
    if world.is_empty() {
        out.push(issue("error", "The server needs a world name."));
    } else if world.contains(BAD_NAME_CHARS) || world.starts_with('.') || world.len() > 64 || world.contains("_backup_") {
        out.push(issue("error", format!("“{world}” can't be used as a world name – it becomes a folder, so avoid \\ / : * ? \" < > | and “_backup_”.")));
    }
    // The server's own rules (manual + `Error bad password:The password is too short` – verified:
    // the server then hangs half-started instead of exiting; only enforced on public servers).
    let pw = &c.password;
    if !pw.is_empty() {
        if pw.chars().count() < 5 {
            if c.public {
                out.push(issue("error", "The password must be at least 5 characters (the server refuses shorter ones)."));
            } else {
                out.push(issue("warn", "The password is shorter than 5 characters – fine while unlisted, but the server refuses it once the server is public."));
            }
        }
        if !name.is_empty() && name.to_lowercase().contains(&pw.to_lowercase()) {
            out.push(issue("error", "The password can't be part of the server name."));
        }
        if pw.contains('"') || pw.contains(char::is_whitespace) {
            out.push(issue("error", "The password can't contain spaces or double quotes."));
        }
    } else if c.public {
        out.push(issue("warn", "No password – anyone who finds the server in the list can join."));
    }
    if c.port < 1024 || c.port == u16::MAX {
        out.push(issue("error", "Pick a port between 1024 and 65534 (the server also uses the next one)."));
    }
    if c.save_interval < 60 {
        out.push(issue("error", "Save at least every 60 seconds or less often (1800 is the default)."));
    } else if c.save_interval > 7200 {
        out.push(issue("warn", "Saving less than every 2 hours risks losing a lot of progress after a crash."));
    }
    if c.backups == 0 {
        out.push(issue("warn", "The server keeps no world backups of its own – only the manager's backups protect the world."));
    }
    if c.backup_short < 60 || c.backup_long < 60 {
        out.push(issue("error", "Backup intervals must be at least 60 seconds."));
    }
    let p = c.preset.trim();
    if !p.is_empty() && !PRESETS.contains(&p) {
        out.push(issue("error", format!("“{p}” isn't a world modifier preset.")));
    }
    for (k, v) in &c.modifiers {
        let v = v.trim();
        if v.is_empty() || v == "default" {
            continue;
        }
        match MODIFIERS.iter().find(|(m, _)| m == k) {
            Some((_, values)) if values.contains(&v) => {}
            Some(_) => out.push(issue("error", format!("“{v}” isn't a valid value for the {k} modifier."))),
            None => out.push(issue("error", format!("Unknown world modifier “{k}”."))),
        }
    }
    for k in &c.keys {
        if !SET_KEYS.contains(&k.as_str()) {
            out.push(issue("error", format!("Unknown world modifier key “{k}”.")));
        }
    }
    if !c.instance_id.trim().is_empty() && !c.instance_id.trim().chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        out.push(issue("error", "The instance id may only use letters, digits, - and _."));
    }
    if c.crossplay && !c.public {
        out.push(issue("info", "Crossplay without a server-list entry: players join with the join code or your public IP."));
    }
    out
}

pub fn ensure_valid(c: &ServerConfig) -> AppResult<()> {
    let errs: Vec<String> = validate(c).into_iter().filter(|i| i.level == "error").map(|i| i.message).collect();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(AppError::msg(format!("This configuration can't be used: {}", errs.join(" "))))
    }
}

/// `-preset` / `-modifier` / `-setkey` arguments (preset first – the manual: "if combined with a
/// preset [modifiers] should be set after").
pub fn modifier_args(c: &ServerConfig) -> Vec<String> {
    let mut v = vec![];
    let p = c.preset.trim();
    if !p.is_empty() {
        v.push("-preset".into());
        v.push(p.to_string());
    }
    for (k, _) in MODIFIERS {
        if let Some(val) = c.modifiers.get(*k).map(|s| s.trim()).filter(|s| !s.is_empty() && *s != "default") {
            v.push("-modifier".into());
            v.push(k.to_string());
            v.push(val.to_string());
        }
    }
    for k in SET_KEYS {
        if c.keys.iter().any(|x| x == k) {
            v.push("-setkey".into());
            v.push(k.to_string());
        }
    }
    v
}

/// Settings from a hand-made start script (`start_headless_server.bat` or a copy of it): the
/// `valheim_server` line's `-name -port -world -password -public -crossplay -savedir …`. Used when
/// an existing install is added. Returns the save folder when the script sets one.
pub fn read_start_script(text: &str, c: &mut ServerConfig) -> Option<String> {
    let line = text.lines().find(|l| l.to_lowercase().contains("valheim_server") && l.contains('-'))?;
    let args = super::server::split_args(line);
    let val = |i: usize| args.get(i + 1).map(|s| s.trim_matches('"').to_string()).filter(|s| !s.starts_with('-'));
    let mut savedir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].to_lowercase().as_str() {
            "-name" => c.name = val(i).unwrap_or_else(|| c.name.clone()),
            "-port" => c.port = val(i).and_then(|v| v.parse().ok()).unwrap_or(c.port),
            "-world" => c.world = val(i).unwrap_or_else(|| c.world.clone()),
            "-password" => c.password = val(i).unwrap_or_default(),
            "-public" => c.public = val(i).map(|v| v != "0").unwrap_or(true),
            "-crossplay" => c.crossplay = true,
            "-instanceid" => c.instance_id = val(i).unwrap_or_default(),
            "-saveinterval" => c.save_interval = val(i).and_then(|v| v.parse().ok()).unwrap_or(c.save_interval),
            "-backups" => c.backups = val(i).and_then(|v| v.parse().ok()).unwrap_or(c.backups),
            "-backupshort" => c.backup_short = val(i).and_then(|v| v.parse().ok()).unwrap_or(c.backup_short),
            "-backuplong" => c.backup_long = val(i).and_then(|v| v.parse().ok()).unwrap_or(c.backup_long),
            "-preset" => c.preset = val(i).map(|v| v.to_lowercase()).unwrap_or_default(),
            "-modifier" => {
                if let (Some(k), Some(v)) = (args.get(i + 1), args.get(i + 2)) {
                    c.modifiers.insert(k.to_lowercase(), v.to_lowercase());
                    i += 1;
                }
            }
            "-setkey" => {
                if let Some(k) = val(i).map(|v| v.to_lowercase()) {
                    if !c.keys.contains(&k) {
                        c.keys.push(k);
                    }
                }
            }
            "-savedir" => savedir = val(i),
            _ => {}
        }
        i += 1;
    }
    savedir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_official_start_script() {
        // start_headless_server.bat as shipped with build 25909431.
        let bat = "@echo off\nset SteamAppId=892970\n\necho \"Starting server PRESS CTRL-C to exit\"\n\nREM Tip: Make a local copy of this script to avoid it being overwritten by steam.\nvalheim_server -nographics -batchmode -name \"My server\" -port 2456 -world \"Dedicated\" -password \"secret\" -crossplay\n";
        let mut c = ServerConfig::default();
        assert_eq!(read_start_script(bat, &mut c), None);
        assert_eq!((c.name.as_str(), c.port, c.world.as_str(), c.password.as_str(), c.crossplay), ("My server", 2456, "Dedicated", "secret", true));
        let mut c = ServerConfig::default();
        let custom = "valheim_server.exe -name Vikings -port 2470 -world Midgard -public 0 -savedir \"D:\\vh data\" -preset hard -modifier raids none -setkey nomap";
        assert_eq!(read_start_script(custom, &mut c).as_deref(), Some("D:\\vh data"));
        assert_eq!((c.port, c.public, c.preset.as_str(), c.modifiers["raids"].as_str(), c.keys.clone()), (2470, false, "hard", "none", vec!["nomap".to_string()]));
    }

    #[test]
    fn lists_keep_comments_and_normalize_ids() {
        let dir = std::env::temp_dir().join(format!("vhsm-list-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(BAN_FILE);
        // As the server writes it (verified), plus an old-style bare SteamID.
        std::fs::write(&p, "// List banned players ID  ONE per line\n76561197960287930\n").unwrap();
        assert_eq!(read_list(&p), vec!["Steam_76561197960287930".to_string()]);
        assert!(edit_list(&p, "76561198000000001", true).unwrap());
        assert!(!edit_list(&p, "Steam_76561198000000001", true).unwrap(), "same player, other spelling");
        assert!(edit_list(&p, "Steam_76561197960287930", false).unwrap(), "removes the bare id too");
        assert!(edit_list(&p, "Xbox_2535412345678901", true).unwrap());
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("// List banned players ID  ONE per line\n"));
        assert_eq!(read_list(&p), vec!["Steam_76561198000000001".to_string(), "Xbox_2535412345678901".to_string()]);
        assert!(edit_list(&p, "12345", true).is_err());
        assert!(edit_list(&dir.join(ADMIN_FILE), "76561198000000009", true).unwrap());
        assert!(std::fs::read_to_string(dir.join(ADMIN_FILE)).unwrap().starts_with("// List admin players"));
        assert_eq!(bad_list_lines("// c\nSteam_76561198000000001\nhello world\n"), vec!["hello world".to_string()]);
        assert_eq!(steam_part("Steam_76561198000000001").as_deref(), Some("76561198000000001"));
        assert_eq!(steam_part("Xbox_123"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validation_mirrors_the_server() {
        let mut c = ServerConfig { password: "secret12".into(), ..Default::default() };
        assert!(validate(&c).iter().all(|i| i.level != "error"), "{:?}", validate(&c));
        c.password = "abc".into();
        assert!(validate(&c).iter().any(|i| i.level == "error" && i.message.contains("at least 5")));
        c.public = false;
        assert!(validate(&c).iter().all(|i| i.level != "error"), "unlisted servers accept short passwords (verified)");
        c.public = true;
        c.password = "server".into();
        c.name = "My Server".into();
        assert!(validate(&c).iter().any(|i| i.message.contains("part of the server name")));
        c.password = "".into();
        assert!(validate(&c).iter().any(|i| i.level == "warn"));
        c.world = "a/b".into();
        assert!(ensure_valid(&c).is_err());
        c.world = "Midgard".into();
        c.preset = "nightmare".into();
        assert!(ensure_valid(&c).is_err());
        c.preset = "hard".into();
        c.modifiers.insert("raids".into(), "none".into());
        c.modifiers.insert("combat".into(), "default".into());
        c.keys = vec!["nomap".into()];
        assert!(ensure_valid(&c).is_ok());
        assert_eq!(modifier_args(&c), vec!["-preset", "hard", "-modifier", "raids", "none", "-setkey", "nomap"]);
        c.modifiers.insert("raids".into(), "lots".into());
        assert!(ensure_valid(&c).is_err());
    }
}
