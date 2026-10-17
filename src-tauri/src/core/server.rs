//! Server lifecycle: launch line, start / stop / restart / update.
//!
//! Every lifecycle operation for one server runs under `AppState::op_lock(id)` so a scheduled
//! restart, a crash restart and a button click can never interleave.

use super::{backup, config, mods, proc, steamcmd};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::state::{manager_log, notify, notify_kind, now_ms, AppState, TaskReporter};
use std::time::Duration;
use tauri::AppHandle;

/// Split a free-form argument string, keeping "quoted values" together.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut in_q = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_q = !in_q;
                cur.push(c);
            }
            c if c.is_whitespace() && !in_q => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', ""))
}

/// The server's command line after the exe. Valheim takes all of its settings here – there is no
/// config file – so the join password has to go on the command line too (`masked` hides it for
/// display). The save folder and the log are absolute, so the working folder never matters.
pub fn build_args_with(inst: &ServerInstance, masked: bool) -> Vec<String> {
    let c = &inst.config;
    let mut v: Vec<String> = vec!["-nographics".into(), "-batchmode".into(), "-name".into(), q(c.name.trim()), "-port".into(), c.port.to_string(), "-world".into(), q(c.world.trim())];
    if !c.password.is_empty() {
        v.push("-password".into());
        v.push(if masked { "\"********\"".into() } else { q(&c.password) });
    }
    v.push("-public".into());
    v.push(if c.public { "1" } else { "0" }.into());
    if c.crossplay {
        v.push("-crossplay".into());
    }
    if !c.instance_id.trim().is_empty() {
        v.push("-instanceid".into());
        v.push(q(c.instance_id.trim()));
    }
    v.extend([
        "-savedir".to_string(),
        q(&inst.data_dir().display().to_string()),
        "-logFile".to_string(),
        q(&inst.log_path().display().to_string()),
        "-saveinterval".to_string(),
        c.save_interval.to_string(),
        "-backups".to_string(),
        c.backups.to_string(),
        "-backupshort".to_string(),
        c.backup_short.to_string(),
        "-backuplong".to_string(),
        c.backup_long.to_string(),
    ]);
    v.extend(config::modifier_args(c));
    v.extend(split_args(&inst.launch.extra_args));
    v
}

pub fn build_args(inst: &ServerInstance) -> Vec<String> {
    build_args_with(inst, false)
}

/// For display: the password is masked.
pub fn launch_line(inst: &ServerInstance) -> String {
    format!("\"{}\" {}", inst.server_exe().display(), build_args_with(inst, true).join(" "))
}

/// Environment the server needs: `SteamAppId` = the game (what `start_headless_server.bat` sets –
/// verified in the log: "Using environment steamid 892970").
pub fn server_env() -> Vec<(String, String)> {
    vec![("SteamAppId".into(), GAME_APP_ID.to_string())]
}

/// The server overwrites its log on every start: keep the previous one in `logs\backup` (last 20).
fn rotate_log(inst: &ServerInstance) {
    let log = inst.log_path();
    let Ok(meta) = std::fs::metadata(&log) else { return };
    if meta.len() == 0 {
        return;
    }
    let dir = inst.logs_dir().join("backup");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let _ = std::fs::rename(&log, dir.join(format!("valheim_server_{stamp}.log")));
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "log").unwrap_or(false)).collect();
        v.sort();
        let n = v.len();
        for p in v.into_iter().take(n.saturating_sub(20)) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// PIDs of this server's running processes and the main PID.
pub fn running_pids(inst: &ServerInstance) -> (Vec<u32>, Option<u32>) {
    let mut sys = sysinfo::System::new();
    let all = proc::scan_servers(&mut sys);
    let mine = proc::procs_for_install(&all, &inst.install_dir);
    (mine.iter().map(|p| p.pid).collect(), mine.first().map(|p| p.pid))
}

pub fn validate_for_start(inst: &ServerInstance) -> AppResult<Vec<String>> {
    config::ensure_valid(&inst.config)?;
    Ok(config::validate(&inst.config).into_iter().filter(|i| i.level == "warn").map(|i| i.message).collect())
}

pub async fn start(state: &AppState, app: &AppHandle, id: &str) -> AppResult<()> {
    let lock = state.op_lock(id).await;
    let _g = lock.lock().await;
    start_locked(state, app, id).await
}

async fn start_locked(state: &AppState, app: &AppHandle, id: &str) -> AppResult<()> {
    let inst = state.instance(id).await?;
    if !inst.is_installed() {
        return Err(AppError::msg("Server files are not installed yet. Click Install first."));
    }
    if let (_, Some(pid)) = running_pids(&inst) {
        state
            .with_runtime(id, |r| {
                r.pid = Some(pid);
                r.state = Some(ServerState::Running);
                r.started_at.get_or_insert(now_ms());
            })
            .await;
        state.emit_status(app, id).await;
        return Ok(());
    }
    for w in validate_for_start(&inst)? {
        manager_log(app, id, format!("Note: {w}"));
    }

    // Another program (or another server) on one of the ports.
    let udp = proc::udp_port_owners().await;
    if let Some((port, pid)) = inst.udp_ports().into_iter().find_map(|p| udp.get(&p).map(|pid| (p, *pid))) {
        let mut sys = sysinfo::System::new();
        let name = proc::process_name(&mut sys, pid).unwrap_or_else(|| "another program".into());
        return Err(AppError::msg(format!("UDP port {port} is already in use by {name} (PID {pid}). Pick a different port under Server Settings.")));
    }

    if inst.automation.update_on_start {
        state.set_busy(app, id, Some("Checking for updates")).await;
        let res = update_files(state, app, &inst, false).await;
        state.set_busy(app, id, None).await;
        if let Err(e) = res {
            notify(app, id, "warn", format!("Update before start failed, starting anyway: {e}"));
        }
    }

    config::ensure_lists(&inst)?;
    std::fs::create_dir_all(inst.worlds_dir()).ok();
    std::fs::create_dir_all(inst.logs_dir()).ok();
    // BepInEx loads through its winhttp.dll proxy; "mods off" parks the proxy for this start.
    match mods::apply_enabled(&inst) {
        Ok(Some(msg)) => manager_log(app, id, msg),
        Ok(None) => {}
        Err(e) => manager_log(app, id, format!("Couldn't switch mods {}: {e}", if inst.launch.mods_enabled { "on" } else { "off" })),
    }
    let args = build_args(&inst);
    let exe = inst.server_exe();
    // The server truncates its log at start: keep the old one and tail the new file from the top.
    rotate_log(&inst);
    let pid = proc::spawn_server(&exe, &args, &server_env(), !inst.launch.show_console, &inst.launch.priority)?;
    if !inst.launch.affinity.is_empty() {
        let _ = proc::apply_priority_affinity(pid, &inst.launch.priority, &inst.launch.affinity);
    }
    state
        .with_runtime(id, |r| {
            r.pid = Some(pid);
            r.state = Some(ServerState::Starting);
            r.started_at = Some(now_ms());
            r.ready_at = None;
            r.intentional_stop = false;
            r.pending_restart = false;
            r.restart_at = None;
            r.online.clear();
            r.query_players = None;
            r.log_offset = 0;
            r.log_partial.clear();
            r.log_sig = None;
            r.console_hidden = !inst.launch.show_console;
            r.log_ready = false;
            r.fatal = None;
            r.game_version = None;
            r.steam_id = None;
            r.public_ip = None;
            r.join_code = None;
            r.modded = false;
            r.last_exit_code = None;
            r.empty_since = None;
            r.waiting_empty = None;
        })
        .await;
    manager_log(app, id, format!("Launching {} (PID {pid}) {}", exe.file_name().unwrap_or_default().to_string_lossy(), build_args_with(&inst, true).join(" ")));
    state.history.event(Some(id), "info", "lifecycle", "Server starting");
    if inst.launch.upnp {
        let (app2, i2) = (app.clone(), inst.clone());
        tauri::async_runtime::spawn(async move {
            match super::upnp::map_server(&i2).await {
                Ok(lines) => manager_log(&app2, &i2.id, format!("UPnP: {}", lines.join(", "))),
                Err(e) => notify(&app2, &i2.id, "warn", format!("UPnP port forwarding failed: {e}")),
            }
        });
    }
    state.emit_status(app, id).await;
    Ok(())
}

/// Graceful stop: Ctrl+Break to the server console (verified: `Game - OnApplicationQuit`, the world
/// is saved – "World save (5/5) done" – and the process exits within ~4 s), force-kill on timeout.
pub async fn stop(state: &AppState, app: &AppHandle, id: &str, force: bool) -> AppResult<()> {
    let lock = state.op_lock(id).await;
    let _g = lock.lock().await;
    stop_locked(state, app, id, force).await
}

async fn stop_locked(state: &AppState, app: &AppHandle, id: &str, force: bool) -> AppResult<()> {
    let inst = state.instance(id).await?;
    let (pids, main) = running_pids(&inst);
    let Some(main) = main else {
        state
            .with_runtime(id, |r| {
                r.pid = None;
                r.state = Some(ServerState::Stopped);
                r.restart_at = None;
                r.online.clear();
            })
            .await;
        state.emit_status(app, id).await;
        return Ok(());
    };
    state
        .with_runtime(id, |r| {
            r.intentional_stop = true;
            r.restart_at = None;
            r.state = Some(ServerState::Stopping);
            r.waiting_empty = None;
        })
        .await;
    state.emit_status(app, id).await;

    let timeout = inst.automation.stop_timeout_secs.clamp(5, 900) as u64;
    let mut exited = false;
    if !force {
        manager_log(app, id, "Asking the server to save and shut down…");
        if proc::request_graceful_exit(main).await {
            let mut sys = sysinfo::System::new();
            for _ in 0..timeout * 2 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if !pids.iter().any(|p| proc::pid_alive(&mut sys, *p)) {
                    exited = true;
                    break;
                }
            }
            if !exited {
                manager_log(app, id, format!("Server did not exit within {timeout}s – forcing it to close."));
                notify(app, id, "warn", format!("{} didn't shut down within {timeout}s and was force-closed.", inst.name));
            }
        } else {
            manager_log(app, id, "Couldn't reach the server console – forcing it to close.");
        }
    }
    if !exited {
        for p in &pids {
            let _ = proc::kill_tree(*p).await;
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    state
        .with_runtime(id, |r| {
            r.pid = None;
            r.state = Some(ServerState::Stopped);
            r.started_at = None;
            r.ready_at = None;
            r.online.clear();
            r.query_players = None;
            r.pending_restart = false;
            r.waiting_empty = None;
            r.join_code = None;
        })
        .await;
    state.history.sessions_close_all(id, now_ms());
    manager_log(app, id, if exited { "Server saved and stopped." } else { "Server process terminated." });
    notify_kind(app, id, "info", "lifecycle", if exited { format!("{} stopped", inst.name) } else { format!("{} was force-stopped", inst.name) });
    state.emit_status(app, id).await;

    if inst.automation.backup_on_stop && inst.worlds_dir().exists() {
        state.set_busy(app, id, Some("Backing up")).await;
        let res = backup::create(state, &inst, "After stop", "auto").await;
        state.set_busy(app, id, None).await;
        match res {
            Ok(b) => {
                state.with_runtime(id, |r| r.last_backup = b.created_at).await;
                let _ = tauri::Emitter::emit(app, "backup-created", &b);
            }
            Err(e) => notify(app, id, "warn", format!("Backup after stop failed: {e}")),
        }
    }
    Ok(())
}

pub async fn restart(state: &AppState, app: &AppHandle, id: &str, update: bool) -> AppResult<()> {
    let lock = state.op_lock(id).await;
    let _g = lock.lock().await;
    let inst = state.instance(id).await?;
    state
        .with_runtime(id, |r| {
            r.state = Some(ServerState::Restarting);
            r.crash_loop = false;
        })
        .await;
    state.emit_status(app, id).await;
    stop_locked(state, app, id, false).await?;
    if update {
        state.with_runtime(id, |r| r.state = Some(ServerState::Updating)).await;
        state.emit_status(app, id).await;
        let res = update_files(state, app, &inst, false).await;
        state.with_runtime(id, |r| r.state = Some(ServerState::Stopped)).await;
        if let Err(e) = res {
            notify(app, id, "error", format!("Update failed, restarting on the current build: {e}"));
        }
    }
    start_locked(state, app, id).await
}

/// SteamCMD install/update of one server's files (caller decides about stopping first).
pub async fn update_files(state: &AppState, app: &AppHandle, inst: &ServerInstance, validate: bool) -> AppResult<()> {
    let title = if !inst.is_installed() {
        format!("Installing {}", inst.name)
    } else if validate {
        format!("Validating {}", inst.name)
    } else {
        format!("Updating {}", inst.name)
    };
    let r = TaskReporter::new(app, "steamcmd", &title, Some(&inst.id));
    let cancel = state.register_cancel(&r.task_id).await;
    let steam_dir = state.steamcmd_dir().await;
    let before = steamcmd::local_build(&inst.install_dir);
    let res = {
        let _steam = state.steam_lock.lock().await;
        steamcmd::app_update(&state.http, &steam_dir, &inst.install_dir, &inst.branch, &inst.branch_password, validate, &r, cancel).await
    };
    state.cancels.lock().await.remove(&r.task_id);
    match res {
        Ok(()) => {
            let build = steamcmd::local_build(&inst.install_dir);
            state.with_runtime(&inst.id, |rt| rt.installed_build = build.clone()).await;
            if let Some(b) = &build {
                state.history.build(&inst.id, b, &inst.branch);
            }
            // Create the permission lists so they can be edited before the first start.
            let _ = config::ensure_lists(inst);
            let b = build.clone().unwrap_or_else(|| "?".into());
            r.done(&format!("{} – build {b}", title.replace("ing ", "ed ")));
            if before.is_some() && before != build {
                state.history.event(Some(&inst.id), "success", "update", &format!("Updated to build {b}"));
                if mods::bepinex_installed(inst) {
                    notify(app, &inst.id, "warn", "The server was updated – check that your mods support the new game version before players join.");
                }
            }
            Ok(())
        }
        Err(e) => {
            r.fail(&e.to_string());
            Err(e)
        }
    }
}

/// Full update job: stops the server if needed (with a backup), updates, restarts if it was up.
pub async fn update_job(state: &AppState, app: &AppHandle, id: &str, validate: bool) -> AppResult<()> {
    let lock = state.op_lock(id).await;
    let _g = lock.lock().await;
    let inst = state.instance(id).await?;
    let (_, main) = running_pids(&inst);
    let was_running = main.is_some();
    if was_running {
        manager_log(app, id, "Stopping the server to update it…");
        stop_locked(state, app, id, false).await?;
    }
    // stop_locked already backs up when "backup on stop" is on; otherwise honour "before update".
    if inst.automation.backup_before_update && !(was_running && inst.automation.backup_on_stop) && inst.worlds_dir().exists() {
        if let Ok(b) = backup::create(state, &inst, "Before update", "auto").await {
            state.with_runtime(id, |r| r.last_backup = b.created_at).await;
        }
    }
    state.with_runtime(id, |r| r.state = Some(if inst.is_installed() { ServerState::Updating } else { ServerState::Installing })).await;
    state.emit_status(app, id).await;
    let res = update_files(state, app, &inst, validate).await;
    state.with_runtime(id, |r| r.state = None).await;
    state.emit_status(app, id).await;
    res?;
    if was_running {
        start_locked(state, app, id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_carry_every_setting() {
        let mut inst = ServerInstance { install_dir: std::path::PathBuf::from(r"D:\vh\s1"), ..Default::default() };
        inst.config.name = "Odin's \"Hall\"".into();
        inst.config.password = "secret12".into();
        let a = build_args(&inst);
        assert_eq!(&a[..8], ["-nographics", "-batchmode", "-name", "\"Odin's Hall\"", "-port", "2456", "-world", "\"Dedicated\""]);
        assert!(a.windows(2).any(|w| w == ["-password", "\"secret12\""]));
        assert!(a.windows(2).any(|w| w == ["-savedir", "\"D:\\vh\\s1\\save-data\""]));
        assert!(a.windows(2).any(|w| w == ["-logFile", "\"D:\\vh\\s1\\logs\\valheim_server.log\""]));
        assert!(!a.contains(&"-crossplay".to_string()));
        assert!(launch_line(&inst).contains("-password \"********\""));
        assert!(!launch_line(&inst).contains("secret12"));
        inst.config.crossplay = true;
        inst.config.public = false;
        inst.config.preset = "hard".into();
        inst.launch.extra_args = "-foo \"bar baz\"".into();
        let a = build_args(&inst);
        assert!(a.windows(2).any(|w| w == ["-public", "0"]));
        assert!(a.contains(&"-crossplay".to_string()));
        assert!(a.windows(2).any(|w| w == ["-preset", "hard"]));
        assert_eq!(a[a.len() - 2..], ["-foo".to_string(), "\"bar baz\"".to_string()]);
        inst.config.password.clear();
        assert!(!build_args(&inst).contains(&"-password".to_string()));
    }

    #[test]
    fn log_rotation_keeps_previous_session() {
        let dir = std::env::temp_dir().join(format!("vhsm-log-{}", uuid::Uuid::new_v4()));
        let inst = ServerInstance { install_dir: dir.clone(), ..Default::default() };
        std::fs::create_dir_all(inst.logs_dir()).unwrap();
        std::fs::write(inst.log_path(), "old session").unwrap();
        rotate_log(&inst);
        assert!(!inst.log_path().exists());
        assert_eq!(std::fs::read_dir(inst.logs_dir().join("backup")).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
