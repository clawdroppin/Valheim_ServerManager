//! Every `#[tauri::command]`. Thin wrappers that delegate to `core::*`.

use crate::core::history::{EventRow, MetricRow, PlayerSummary, SessionRow};
use crate::core::{backup, config, diagnostics, mods, network, presets, proc, server, steamcmd, supervisor, worlds};
use crate::error::{AppError, AppResult};
use crate::models::*;
use crate::state::{manager_log, now_ms, AppState, TaskReporter};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

type St<'a> = State<'a, Arc<AppState>>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::msg(e.to_string()))?
}

/// Run a long job in the background; errors become toasts. Returns immediately.
fn background<F>(app: &AppHandle, id: &str, what: &'static str, fut: F)
where
    F: std::future::Future<Output = AppResult<()>> + Send + 'static,
{
    let (app2, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = fut.await {
            crate::state::notify(&app2, &id, "error", format!("{what} failed: {e}"));
        }
    });
}

fn require_stopped(inst: &ServerInstance) -> AppResult<()> {
    let (_, main) = server::running_pids(inst);
    if main.is_some() {
        return Err(AppError::msg("Stop the server first – it has these files open while running."));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------- app

#[tauri::command]
pub async fn get_app_info(state: St<'_>) -> AppResult<AppInfo> {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.data_dir.clone(),
        portable: state.portable,
        app_id: SERVER_APP_ID,
        cores: sys.cpus().len(),
        total_memory: sys.total_memory(),
        os: sysinfo::System::long_os_version().unwrap_or_else(|| "Windows".into()),
        hostname: sysinfo::System::host_name().unwrap_or_default(),
        lost_secrets: state.lost_secrets.clone(),
    })
}

#[tauri::command]
pub async fn get_settings(state: St<'_>) -> AppResult<AppSettings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(state: St<'_>, settings: AppSettings) -> AppResult<()> {
    let mut s = settings;
    s.last_data_dir = Some(state.data_dir.clone());
    s.metrics_retention_days = s.metrics_retention_days.clamp(1, 365);
    *state.settings.write().await = s;
    state.persist_settings().await
}

#[tauri::command]
pub async fn test_webhook(state: St<'_>, url: String) -> AppResult<()> {
    crate::state::send_webhook(&state.http, url.trim(), "success", "", "Test message from Valheim Server Manager \u{2013} notifications work!").await
}

/// A strong random join password (letters and digits, nothing that's easy to mistype).
#[tauri::command]
pub fn generate_password(len: Option<usize>) -> String {
    crate::core::secrets::random_password(len.unwrap_or(10).clamp(5, 32))
}

#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn window_hide(window: tauri::WebviewWindow) {
    let _ = window.hide();
}

#[tauri::command]
pub async fn cancel_task(state: St<'_>, task_id: String) -> AppResult<()> {
    if let Some(f) = state.cancels.lock().await.get(&task_id) {
        f.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderProbe {
    exists: bool,
    writable: bool,
    empty: bool,
    has_server: bool,
    free_bytes: Option<u64>,
}

#[tauri::command]
pub async fn probe_folder(path: String) -> AppResult<FolderProbe> {
    blocking(move || {
        let p = PathBuf::from(&path);
        let exists = p.exists();
        let empty = !exists || std::fs::read_dir(&p).map(|mut r| r.next().is_none()).unwrap_or(true);
        let has_server = resolve_server_exe(&p).exists();
        let probe_dir = if exists { p.clone() } else { p.ancestors().find(|a| a.exists()).map(|a| a.to_path_buf()).unwrap_or(p.clone()) };
        let t = probe_dir.join(format!(".vhsm-{}", uuid::Uuid::new_v4()));
        let writable = std::fs::write(&t, b"").is_ok();
        let _ = std::fs::remove_file(&t);
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let n = proc::norm(&probe_dir);
        let free_bytes = disks
            .iter()
            .filter(|d| n.starts_with(&proc::norm(d.mount_point())))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map(|d| d.available_space());
        Ok(FolderProbe { exists, writable, empty, has_server, free_bytes })
    })
    .await
}

// ---------------------------------------------------------------------------------- instances

#[tauri::command]
pub async fn list_instances(state: St<'_>) -> AppResult<Vec<ServerInstance>> {
    Ok(state.instances.read().await.clone())
}

#[tauri::command]
pub async fn get_statuses(state: St<'_>) -> AppResult<Vec<ServerStatus>> {
    let list = state.instances.read().await.clone();
    let mut out = vec![];
    for i in &list {
        out.push(state.status(i).await);
    }
    Ok(out)
}

#[tauri::command]
pub async fn suggest_ports(state: St<'_>, exclude_id: Option<String>) -> AppResult<network::PortSet> {
    let list = state.instances.read().await.clone();
    Ok(network::suggest_ports(&list, exclude_id.as_deref()).await)
}

fn ports_clash(list: &[ServerInstance], inst: &ServerInstance) -> bool {
    let mine = inst.udp_ports();
    list.iter().filter(|o| o.id != inst.id).any(|o| o.udp_ports().iter().any(|p| mine.contains(p)))
}

async fn assign_free_ports(state: &AppState, inst: &mut ServerInstance) {
    let list = state.instances.read().await.clone();
    if inst.config.port == 0 || ports_clash(&list, inst) {
        inst.config.port = network::suggest_ports(&list, Some(&inst.id)).await.port;
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    name: String,
    install_dir: Option<PathBuf>,
    color: Option<String>,
    config: ServerConfig,
    install_now: bool,
    template_id: Option<String>,
}

/// Folders must be absolute – a relative path would resolve against wherever the app (or
/// SteamCMD, which then silently falls back to its own library) happens to run.
fn abs_dir(p: PathBuf) -> AppResult<PathBuf> {
    let s = p.to_string_lossy().trim().replace('/', "\\").trim_end_matches('\\').to_string();
    let s = if s.len() == 2 && s.ends_with(':') { format!("{s}\\") } else { s };
    let p = PathBuf::from(&s);
    if !p.is_absolute() {
        return Err(AppError::msg(format!("“{s}” isn't a full folder path – use something like D:\\ValheimServers\\MyServer.")));
    }
    Ok(p)
}

fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "server".into()
    } else {
        s.chars().take(40).collect()
    }
}

async fn unique_dir(state: &AppState, name: &str) -> PathBuf {
    let root = state.default_install_root().await;
    let base = slug(name);
    let mut p = root.join(&base);
    let mut n = 2;
    let used: Vec<String> = state.instances.read().await.iter().map(|i| proc::norm(&i.install_dir)).collect();
    while p.exists() && std::fs::read_dir(&p).map(|mut r| r.next().is_some()).unwrap_or(false) || used.contains(&proc::norm(&p)) {
        p = root.join(format!("{base}-{n}"));
        n += 1;
    }
    p
}

async fn add_instance(state: &AppState, inst: ServerInstance) -> AppResult<ServerInstance> {
    {
        let mut list = state.instances.write().await;
        if list.iter().any(|i| proc::norm(&i.install_dir) == proc::norm(&inst.install_dir)) {
            return Err(AppError::msg("Another server profile already uses that folder."));
        }
        list.push(inst.clone());
    }
    state.persist_instances().await?;
    Ok(inst)
}

/// A server folder that already has a save folder keeps using it; a server that was run by hand
/// without `-savedir` keeps its worlds in the default location (`…\LocalLow\IronGate\Valheim`, per
/// the manual). A start script with `-savedir` wins.
fn detect_data_dir(inst: &mut ServerInstance, script_savedir: Option<String>) {
    if let Some(d) = script_savedir.filter(|d| !d.trim().is_empty()) {
        inst.launch.data_dir = d;
        return;
    }
    if inst.worlds_dir().exists() {
        return;
    }
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let def = home.join("AppData").join("LocalLow").join("IronGate").join("Valheim");
        if def.join(WORLDS_DIR).exists() && def.join(ADMIN_FILE).exists() {
            inst.launch.data_dir = def.to_string_lossy().into_owned();
        }
    }
}

/// Settings from the folder's start scripts (`*.bat` that launch valheim_server).
fn read_scripts(dir: &std::path::Path, c: &mut ServerConfig) -> Option<String> {
    let mut bats: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("bat")).unwrap_or(false)).collect();
    // A user's own copy beats the shipped original (which Steam resets on update).
    bats.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy() == "start_headless_server.bat").unwrap_or(false));
    for b in bats {
        let text = String::from_utf8_lossy(&std::fs::read(&b).ok()?).into_owned();
        if text.to_lowercase().contains("valheim_server") {
            return config::read_start_script(&text, c);
        }
    }
    None
}

#[tauri::command]
pub async fn create_instance(app: AppHandle, state: St<'_>, input: CreateInput) -> AppResult<ServerInstance> {
    let name = if input.name.trim().is_empty() { "Valheim Server".to_string() } else { input.name.trim().to_string() };
    let mut inst = match &input.template_id {
        Some(t) => state.instance(t).await?,
        None => ServerInstance::default(),
    };
    inst.id = uuid::Uuid::new_v4().to_string();
    inst.name = name.clone();
    inst.created_at = now_ms();
    inst.color = input.color.unwrap_or_else(|| "fjord".into());
    inst.install_dir = match input.install_dir {
        Some(d) if !d.as_os_str().is_empty() => abs_dir(d)?,
        _ => unique_dir(&state, &name).await,
    };
    inst.config = input.config;
    assign_free_ports(&state, &mut inst).await;
    std::fs::create_dir_all(&inst.install_dir)?;
    config::ensure_valid(&inst.config)?;
    if inst.is_installed() {
        let _ = config::ensure_lists(&inst);
    }
    let inst = add_instance(&state, inst).await?;
    if input.install_now {
        let (st, app2, id) = (state.inner().clone(), app.clone(), inst.id.clone());
        background(&app, &inst.id, "Install", async move { server::update_job(&st, &app2, &id, false).await });
    }
    state.emit_status(&app, &inst.id).await;
    Ok(inst)
}

#[tauri::command]
pub async fn update_instance(app: AppHandle, state: St<'_>, instance: ServerInstance) -> AppResult<ServerInstance> {
    let old = state.instance(&instance.id).await?;
    let mut inst = instance;
    inst.install_dir = abs_dir(inst.install_dir)?;
    if inst.config.port == 0 {
        return Err(AppError::msg("The port can't be 0."));
    }
    config::ensure_valid(&inst.config)?;
    if proc::norm(&old.install_dir) != proc::norm(&inst.install_dir) {
        require_stopped(&old).map_err(|_| AppError::msg("Stop the server before changing its folder."))?;
    }
    state.put_instance(&inst).await?;
    let build = steamcmd::local_build(&inst.install_dir);
    state.with_runtime(&inst.id, |r| r.installed_build = build).await;
    let (_, main) = server::running_pids(&inst);
    // Everything Valheim knows comes from its command line – changes apply at the next start.
    let restart_needed =
        old.config != inst.config || old.launch.extra_args != inst.launch.extra_args || old.launch.data_dir != inst.launch.data_dir || old.launch.mods_enabled != inst.launch.mods_enabled;
    if let Some(pid) = main {
        // Priority / affinity / console visibility apply live.
        if old.launch.priority != inst.launch.priority || old.launch.affinity != inst.launch.affinity {
            let _ = proc::apply_priority_affinity(pid, &inst.launch.priority, &inst.launch.affinity);
        }
        if old.launch.show_console != inst.launch.show_console {
            let (pids, _) = server::running_pids(&inst);
            proc::set_console_visible(&pids, inst.launch.show_console);
            state.with_runtime(&inst.id, |r| r.console_hidden = !inst.launch.show_console).await;
        }
        if restart_needed {
            state.with_runtime(&inst.id, |r| r.pending_restart = true).await;
        }
    }
    if old.config != inst.config {
        state.history.event(Some(&inst.id), "info", "config", "Server settings changed");
    }
    state.emit_status(&app, &inst.id).await;
    let _ = app.emit("instances-changed", ());
    Ok(inst)
}

#[tauri::command]
pub async fn validate_config(config: ServerConfig) -> AppResult<Vec<config::Issue>> {
    Ok(config::validate(&config))
}

#[tauri::command]
pub async fn reorder_instances(state: St<'_>, ids: Vec<String>) -> AppResult<()> {
    {
        let mut list = state.instances.write().await;
        list.sort_by_key(|i| ids.iter().position(|x| x == &i.id).unwrap_or(usize::MAX));
    }
    state.persist_instances().await
}

#[tauri::command]
pub async fn delete_instance(app: AppHandle, state: St<'_>, id: String, delete_files: bool) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    let (_, main) = server::running_pids(&inst);
    if main.is_some() {
        server::stop(&state, &app, &id, false).await?;
    }
    state.instances.write().await.retain(|i| i.id != id);
    state.persist_instances().await?;
    state.runtime.lock().await.remove(&id);
    state.history.forget(&id);
    if delete_files && inst.install_dir.exists() {
        // Keep backups: they live inside the folder by default, so move them out first.
        let dir = inst.install_dir.clone();
        let keep = state.data_dir.join("deleted-server-backups").join(&id);
        blocking(move || {
            let bk = dir.join("Backups");
            if bk.exists() {
                std::fs::create_dir_all(keep.parent().unwrap())?;
                let _ = std::fs::rename(&bk, &keep);
            }
            std::fs::remove_dir_all(&dir)?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn clone_instance(state: St<'_>, id: String, name: String, copy_saves: bool) -> AppResult<ServerInstance> {
    let src = state.instance(&id).await?;
    let mut inst = src.clone();
    inst.id = uuid::Uuid::new_v4().to_string();
    inst.name = if name.trim().is_empty() { format!("{} (copy)", src.name) } else { name.trim().to_string() };
    inst.config.name = inst.name.clone();
    inst.created_at = now_ms();
    inst.install_dir = unique_dir(&state, &inst.name).await;
    inst.launch.data_dir = "./save-data".into();
    inst.config.port = 0;
    // A crossplay copy needs its own PlayFab identity.
    inst.config.instance_id.clear();
    assign_free_ports(&state, &mut inst).await;
    let (s2, d2) = (src.clone(), inst.clone());
    blocking(move || {
        std::fs::create_dir_all(d2.data_dir())?;
        // Start from the original's permission lists.
        for f in [ADMIN_FILE, BAN_FILE, PERMIT_FILE] {
            let p = s2.data_dir().join(f);
            if p.exists() {
                std::fs::copy(&p, d2.data_dir().join(f))?;
            }
        }
        if copy_saves {
            for w in worlds::list(&s2) {
                worlds::export(&s2, &w.name, &d2.worlds_dir())?;
            }
        }
        Ok(())
    })
    .await?;
    add_instance(&state, inst).await
}

#[tauri::command]
pub async fn find_existing_installs(state: St<'_>) -> AppResult<Vec<PathBuf>> {
    let known: Vec<String> = state.instances.read().await.iter().map(|i| proc::norm(&i.install_dir)).collect();
    Ok(steamcmd::find_steam_client_installs().await.into_iter().filter(|p| !known.contains(&proc::norm(p))).collect())
}

#[tauri::command]
pub async fn import_existing(app: AppHandle, state: St<'_>, path: PathBuf, name: Option<String>) -> AppResult<ServerInstance> {
    let path = abs_dir(path)?;
    if !resolve_server_exe(&path).exists() {
        return Err(AppError::msg("No Valheim dedicated server found in that folder (looked for valheim_server.exe)."));
    }
    let mut inst = ServerInstance { id: uuid::Uuid::new_v4().to_string(), created_at: now_ms(), install_dir: path.clone(), ..Default::default() };
    // The settings people put into their start script come along.
    let savedir = read_scripts(&path, &mut inst.config);
    detect_data_dir(&mut inst, savedir);
    if config::ensure_valid(&inst.config).is_err() {
        // A script password the server would refuse (too short, part of the name): keep the
        // other values but drop it so the profile is usable; the user sets a new one.
        inst.config.password.clear();
    }
    inst.name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| inst.config.name.clone());
    assign_free_ports(&state, &mut inst).await;
    let inst = add_instance(&state, inst).await?;
    let build = steamcmd::local_build(&inst.install_dir);
    state.with_runtime(&inst.id, |r| r.installed_build = build).await;
    state.emit_status(&app, &inst.id).await;
    Ok(inst)
}

// ---------------------------------------------------------------------------------- lifecycle

#[tauri::command]
pub async fn install_server(app: AppHandle, state: St<'_>, id: String, validate: bool) -> AppResult<()> {
    let st = state.inner().clone();
    let (app2, id2) = (app.clone(), id.clone());
    background(&app, &id, if validate { "Validate" } else { "Update" }, async move { server::update_job(&st, &app2, &id2, validate).await });
    Ok(())
}

#[tauri::command]
pub async fn check_update(app: AppHandle, state: St<'_>, id: String) -> AppResult<bool> {
    let st = state.inner().clone();
    state.latest_build_cache.lock().await.clear();
    supervisor::check_update(&st, &app, &id, false).await
}

#[tauri::command]
pub async fn start_server(app: AppHandle, state: St<'_>, id: String) -> AppResult<()> {
    state.with_runtime(&id, |r| r.crash_loop = false).await;
    server::start(&state, &app, &id).await
}

#[tauri::command]
pub async fn stop_server(app: AppHandle, state: St<'_>, id: String, force: bool) -> AppResult<()> {
    let st = state.inner().clone();
    let (app2, id2) = (app.clone(), id.clone());
    // Graceful stops take a few seconds (world save) – return at once, status events follow.
    background(&app, &id, "Stop", async move { server::stop(&st, &app2, &id2, force).await });
    Ok(())
}

#[tauri::command]
pub async fn restart_server(app: AppHandle, state: St<'_>, id: String, update: bool) -> AppResult<()> {
    let st = state.inner().clone();
    let (app2, id2) = (app.clone(), id.clone());
    background(&app, &id, "Restart", async move { server::restart(&st, &app2, &id2, update).await });
    Ok(())
}

#[tauri::command]
pub async fn cancel_deferred(app: AppHandle, state: St<'_>, id: String) -> AppResult<()> {
    state.with_runtime(&id, |r| r.waiting_empty = None).await;
    manager_log(&app, &id, "Cancelled the pending automatic action.");
    state.emit_status(&app, &id).await;
    Ok(())
}

#[tauri::command]
pub async fn get_launch_line(state: St<'_>, id: String) -> AppResult<String> {
    Ok(server::launch_line(&state.instance(&id).await?))
}

#[tauri::command]
pub async fn set_console_visible(state: St<'_>, id: String, visible: bool) -> AppResult<usize> {
    let inst = state.instance(&id).await?;
    let (pids, _) = server::running_pids(&inst);
    state.with_runtime(&id, |r| r.console_hidden = !visible).await;
    Ok(proc::set_console_visible(&pids, visible))
}

#[tauri::command]
pub async fn clear_crash_state(app: AppHandle, state: St<'_>, id: String) -> AppResult<()> {
    state
        .with_runtime(&id, |r| {
            r.crash_loop = false;
            r.crash_times.clear();
            r.restart_at = None;
            if r.state == Some(ServerState::Crashed) {
                r.state = Some(ServerState::Stopped);
            }
        })
        .await;
    state.emit_status(&app, &id).await;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    info: Option<crate::core::a2s::A2sInfo>,
    players: Vec<crate::core::a2s::A2sPlayer>,
}

/// Ask the server what it tells the Steam browser (A2S) – on this PC, or at an address.
#[tauri::command]
pub async fn query_server(state: St<'_>, id: String, address: Option<String>) -> AppResult<QueryResult> {
    use crate::core::a2s;
    let inst = state.instance(&id).await?;
    let addr: std::net::SocketAddr = match address.filter(|a| !a.trim().is_empty()) {
        Some(a) => {
            // "host" alone means the server's query port.
            let a = a.trim();
            let target = if a.parse::<std::net::SocketAddr>().is_ok() || (a.matches(':').count() == 1 && !a.ends_with(':')) { a.to_string() } else { format!("{a}:{}", inst.config.query_port()) };
            let found = tokio::net::lookup_host(target.clone()).await.map_err(|e| AppError::msg(format!("Couldn't resolve {target}: {e}")))?.next();
            found.ok_or_else(|| AppError::msg("Address not found"))?
        }
        None => a2s::local_addr("", inst.config.query_port()),
    };
    let t = std::time::Duration::from_secs(3);
    let info = a2s::info(addr, t).await;
    let players = if info.is_some() { a2s::players(addr, t).await.unwrap_or_default() } else { vec![] };
    Ok(QueryResult { info, players })
}

// --------------------------------------------------------------------------- permission lists

/// The files the Config Files view edits: admins | bans | permitted.
fn list_file(inst: &ServerInstance, file: &str) -> AppResult<(PathBuf, &'static str)> {
    Ok(match file {
        "admins" => (inst.admin_path(), ADMIN_FILE),
        "bans" => (inst.ban_path(), BAN_FILE),
        "permitted" => (inst.permit_path(), PERMIT_FILE),
        _ => return Err(AppError::msg("Unknown list")),
    })
}

#[tauri::command]
pub async fn read_config(state: St<'_>, id: String, file: String) -> AppResult<ConfigFile> {
    let inst = state.instance(&id).await?;
    let (path, name) = list_file(&inst, &file)?;
    blocking(move || {
        let meta = std::fs::metadata(&path).ok();
        // A file the server hasn't written yet shows the header it would write.
        let text = match std::fs::read(&path) {
            Ok(b) => String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string(),
            Err(_) => format!("{}\n", config::list_header(name)),
        };
        Ok(ConfigFile {
            exists: meta.is_some(),
            modified: meta.and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()),
            text,
            pending: false,
            path,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    restart_needed: bool,
    instance: ServerInstance,
}

/// Save hand edits of a permission list (the server re-reads them while running).
#[tauri::command]
pub async fn write_config(app: AppHandle, state: St<'_>, id: String, file: String, text: String) -> AppResult<WriteResult> {
    let inst = state.instance(&id).await?;
    let (path, _) = list_file(&inst, &file)?;
    let bad = config::bad_list_lines(&text);
    if !bad.is_empty() {
        return Err(AppError::msg(format!("These lines aren't player ids (Steam_7656… / a SteamID64 / Xbox_…): {}", bad.join(", "))));
    }
    let (p, t) = (path.clone(), text.clone());
    blocking(move || config::write_text(&p, &t)).await?;
    let _ = app.emit("access-changed", &id);
    Ok(WriteResult { restart_needed: false, instance: inst })
}

#[tauri::command]
pub async fn list_config_history(state: St<'_>, id: String, file: String) -> AppResult<Vec<(String, i64)>> {
    let inst = state.instance(&id).await?;
    let (path, _) = list_file(&inst, &file)?;
    history_of(path).await
}

async fn history_of(path: PathBuf) -> AppResult<Vec<(String, i64)>> {
    let dir = config::history_dir(&path);
    let prefix = format!("{}.", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    blocking(move || {
        let mut v: Vec<(String, i64)> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                    .map(|e| {
                        let t = e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()).unwrap_or(0);
                        (e.path().to_string_lossy().into_owned(), t)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(v)
    })
    .await
}

#[tauri::command]
pub async fn read_text_file(state: St<'_>, id: String, path: PathBuf) -> AppResult<String> {
    let inst = state.instance(&id).await?;
    if !proc::is_within(&path, &inst.install_dir) && !proc::is_within(&path, &inst.data_dir()) {
        return Err(AppError::msg("That file is outside the server's folders."));
    }
    blocking(move || Ok(String::from_utf8_lossy(&std::fs::read(&path)?).into_owned())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    /// `Steam_7656…` / `Xbox_…` as written in the file.
    id: String,
    steam_id: Option<String>,
    /// Last character name seen with this id (player history).
    name: Option<String>,
    online: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessView {
    admins: Vec<AccessEntry>,
    bans: Vec<AccessEntry>,
    permitted: Vec<AccessEntry>,
}

#[tauri::command]
pub async fn get_access(state: St<'_>, id: String) -> AppResult<AccessView> {
    let inst = state.instance(&id).await?;
    let known = state.history.players(&id);
    let online: Vec<(Option<String>, Option<String>)> = state.with_runtime(&id, |r| r.online.values().map(|o| (o.steam_id.clone(), o.platform_id.clone())).collect()).await;
    let entry = |pid: String| {
        let sid = config::steam_part(&pid);
        AccessEntry {
            name: sid.as_ref().and_then(|s| known.iter().filter(|p| p.steam_id.as_deref() == Some(s.as_str())).max_by_key(|p| p.last_seen).map(|p| p.name.clone())),
            online: online.iter().any(|(s, p)| (sid.is_some() && s == &sid) || p.as_deref() == Some(pid.as_str())),
            steam_id: sid,
            id: pid,
        }
    };
    let (a, b, c) = (inst.admin_path(), inst.ban_path(), inst.permit_path());
    let (admins, bans, permitted) = blocking(move || Ok((config::read_list(&a), config::read_list(&b), config::read_list(&c)))).await?;
    Ok(AccessView { admins: admins.into_iter().map(entry).collect(), bans: bans.into_iter().map(entry).collect(), permitted: permitted.into_iter().map(entry).collect() })
}

/// Add or remove a player id in adminlist.txt / bannedlist.txt / permittedlist.txt.
#[tauri::command]
pub async fn set_access(app: AppHandle, state: St<'_>, id: String, list: String, player_id: String, add: bool) -> AppResult<bool> {
    let inst = state.instance(&id).await?;
    let (path, _) = list_file(&inst, &list)?;
    let pid = player_id.trim().to_string();
    let p2 = pid.clone();
    let changed = blocking(move || config::edit_list(&path, &p2, add)).await?;
    if changed {
        let what = match (list.as_str(), add) {
            ("admins", true) => "Admin added",
            ("admins", false) => "Admin removed",
            ("bans", true) => "Player banned",
            ("bans", false) => "Player unbanned",
            (_, true) => "Player allowed",
            _ => "Player removed from the allow list",
        };
        state.history.event(Some(&id), "info", "config", &format!("{what}: {pid}"));
        let _ = app.emit("access-changed", &id);
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------------------- logs

#[tauri::command]
pub async fn read_log_tail(state: St<'_>, id: String, max_lines: usize) -> AppResult<Vec<String>> {
    let inst = state.instance(&id).await?;
    blocking(move || {
        let path = inst.log_path();
        let Ok(meta) = std::fs::metadata(&path) else { return Ok(vec![]) };
        use std::io::{Read, Seek, SeekFrom};
        let len = meta.len();
        let start = len.saturating_sub(1_048_576);
        let mut f = std::fs::File::open(&path)?;
        f.seek(SeekFrom::Start(start))?;
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let skip = lines.len().saturating_sub(max_lines.clamp(10, 20_000));
        Ok(lines[skip..].to_vec())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    path: PathBuf,
    name: String,
    size: u64,
    modified: i64,
}

/// The current log plus the previous sessions kept in `logs\backup`.
#[tauri::command]
pub async fn list_log_files(state: St<'_>, id: String) -> AppResult<Vec<LogFile>> {
    let inst = state.instance(&id).await?;
    blocking(move || {
        let mut v: Vec<LogFile> = vec![];
        for dir in [inst.logs_dir(), inst.logs_dir().join("backup")] {
            let Ok(rd) = std::fs::read_dir(dir) else { continue };
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if !n.starts_with("valheim_server") || !n.ends_with(".log") {
                    continue;
                }
                let Ok(m) = e.metadata() else { continue };
                v.push(LogFile {
                    name: n,
                    path: e.path(),
                    size: m.len(),
                    modified: m.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()).unwrap_or(0),
                });
            }
        }
        // BepInEx's own log, when mods ran.
        let bx = inst.bepinex_dir().join("LogOutput.log");
        if let Ok(m) = std::fs::metadata(&bx) {
            v.push(LogFile { name: "BepInEx LogOutput.log".into(), path: bx, size: m.len(), modified: m.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()).unwrap_or(0) });
        }
        v.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(v)
    })
    .await
}

// -------------------------------------------------------------------------------------- worlds

#[tauri::command]
pub async fn list_worlds(state: St<'_>, id: String) -> AppResult<Vec<worlds::World>> {
    let inst = state.instance(&id).await?;
    blocking(move || Ok(worlds::list(&inst))).await
}

#[tauri::command]
pub async fn list_local_worlds() -> AppResult<Vec<worlds::LocalWorld>> {
    blocking(|| Ok(worlds::local_worlds())).await
}

/// Worlds in any folder the user picks (an old server's save folder, a copied world).
#[tauri::command]
pub async fn scan_world_folder(path: PathBuf) -> AppResult<Vec<worlds::World>> {
    blocking(move || Ok(worlds::scan_any(&path))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    name: String,
    files: usize,
}

/// Copy a world in (`path` = a world folder or a legacy `.fwl`); `activate` makes it the one the
/// server loads.
#[tauri::command]
pub async fn import_world(app: AppHandle, state: St<'_>, id: String, path: PathBuf, name: String, activate: bool) -> AppResult<ImportResult> {
    let inst = state.instance(&id).await?;
    let i2 = inst.clone();
    let (world, files) = blocking(move || worlds::import(&i2, &path, &name)).await?;
    if activate {
        let mut updated = state.instance(&id).await?;
        updated.config.world = world.clone();
        state.put_instance(&updated).await?;
        if server::running_pids(&updated).1.is_some() {
            state.with_runtime(&id, |r| r.pending_restart = true).await;
        }
        let _ = app.emit("instances-changed", ());
        state.emit_status(&app, &id).await;
    }
    crate::state::notify_kind(&app, &id, "success", "world", format!("Imported world “{world}” ({files} files){}", if activate { " – it loads on the next start" } else { "" }));
    Ok(ImportResult { name: world, files })
}

/// Replace a world with one of the server's own backups of it.
#[tauri::command]
pub async fn restore_world_backup(state: St<'_>, id: String, world: String, backup: String) -> AppResult<String> {
    let inst = state.instance(&id).await?;
    // Held so a start (button, schedule, crash restart) can't race the file changes.
    let lock = state.op_lock(&id).await;
    let _g = lock.lock().await;
    require_stopped(&inst)?;
    let _ = backup::create(&state, &inst, "Before world rollback", "auto").await;
    blocking(move || worlds::restore_backup(&inst, &world, &backup)).await
}

#[tauri::command]
pub async fn export_world(state: St<'_>, id: String, world: String, dest: PathBuf) -> AppResult<usize> {
    let inst = state.instance(&id).await?;
    blocking(move || worlds::export(&inst, &world, &dest)).await
}

#[tauri::command]
pub async fn duplicate_world(state: St<'_>, id: String, world: String, new_name: String) -> AppResult<String> {
    let inst = state.instance(&id).await?;
    blocking(move || worlds::duplicate(&inst, &world, &new_name)).await
}

#[tauri::command]
pub async fn delete_world(state: St<'_>, id: String, world: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    if world.trim().eq_ignore_ascii_case(inst.config.world.trim()) {
        require_stopped(&inst)?;
    }
    blocking(move || worlds::remove(&inst, &world).map(|_| ())).await
}

#[tauri::command]
pub async fn reset_world(state: St<'_>, id: String) -> AppResult<Option<PathBuf>> {
    let inst = state.instance(&id).await?;
    let lock = state.op_lock(&id).await;
    let _g = lock.lock().await;
    require_stopped(&inst)?;
    let _ = backup::create(&state, &inst, "Before world reset", "auto").await;
    blocking(move || worlds::reset(&inst)).await
}

/// Switch the world the server loads (`-world`) – an existing world or a new name (generated on
/// the next start).
#[tauri::command]
pub async fn activate_world(app: AppHandle, state: St<'_>, id: String, name: String) -> AppResult<ServerInstance> {
    let mut inst = state.instance(&id).await?;
    inst.config.world = worlds::check_name(&name)?.to_string();
    state.put_instance(&inst).await?;
    if server::running_pids(&inst).1.is_some() {
        state.with_runtime(&id, |r| r.pending_restart = true).await;
    }
    let _ = app.emit("instances-changed", ());
    state.emit_status(&app, &id).await;
    Ok(inst)
}

/// Rename a world. Renaming the active one moves `-world` along (server stopped).
#[tauri::command]
pub async fn rename_world(app: AppHandle, state: St<'_>, id: String, world: String, new_name: String) -> AppResult<String> {
    let inst = state.instance(&id).await?;
    let lock = state.op_lock(&id).await;
    let _g = lock.lock().await;
    let active = world.trim().eq_ignore_ascii_case(inst.config.world.trim());
    if active {
        require_stopped(&inst)?;
    }
    let i2 = inst.clone();
    let name = blocking(move || worlds::rename(&i2, &world, &new_name)).await?;
    if active {
        let mut updated = state.instance(&id).await?;
        updated.config.world = name.clone();
        state.put_instance(&updated).await?;
        let _ = app.emit("instances-changed", ());
        state.emit_status(&app, &id).await;
    }
    Ok(name)
}

/// Unpack the current save and every server backup of a world to find damaged ones.
#[tauri::command]
pub async fn check_world(state: St<'_>, id: String, world: String) -> AppResult<Vec<worlds::SaveCheck>> {
    let inst = state.instance(&id).await?;
    blocking(move || worlds::check(&inst, &world)).await
}

// ------------------------------------------------------------------------------ rule presets

#[tauri::command]
pub async fn list_rule_presets(state: St<'_>) -> AppResult<Vec<presets::RulePreset>> {
    Ok(presets::list(&state.data_dir))
}

#[tauri::command]
pub async fn save_rule_preset(state: St<'_>, name: String, settings: std::collections::BTreeMap<String, serde_json::Value>) -> AppResult<presets::RulePreset> {
    presets::save(&state.data_dir, &name, settings, now_ms())
}

#[tauri::command]
pub async fn delete_rule_preset(state: St<'_>, name: String) -> AppResult<()> {
    presets::delete(&state.data_dir, &name)
}

// ---------------------------------------------------------------------------------------- mods

#[tauri::command]
pub async fn mods_status(state: St<'_>, id: String) -> AppResult<mods::ModsStatus> {
    let inst = state.instance(&id).await?;
    blocking(move || Ok(mods::status(&inst))).await
}

/// Download BepInExPack_Valheim into the server folder (background task).
#[tauri::command]
pub async fn install_bepinex(app: AppHandle, state: St<'_>, id: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    let st = state.inner().clone();
    let app2 = app.clone();
    background(&app, &id, "Installing BepInEx", async move {
        let lock = st.op_lock(&inst.id).await;
        let _g = lock.lock().await;
        let r = TaskReporter::new(&app2, "mods", "Installing BepInEx", Some(&inst.id));
        match mods::install_bepinex(&st.http, &inst, &st.data_dir.join("tmp"), &r).await {
            Ok(v) => {
                r.done(&format!("BepInEx {v} installed"));
                st.history.event(Some(&inst.id), "success", "mods", &format!("BepInEx {v} installed"));
                let _ = app2.emit("mods-changed", &inst.id);
                Ok(())
            }
            Err(e) => {
                r.fail(&e.to_string());
                Err(e)
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn uninstall_bepinex(app: AppHandle, state: St<'_>, id: String) -> AppResult<PathBuf> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    let p = blocking(move || mods::uninstall_bepinex(&inst)).await?;
    state.history.event(Some(&id), "info", "mods", "BepInEx removed – the server runs vanilla");
    let _ = app.emit("mods-changed", &id);
    Ok(p)
}

/// Install a Thunderstore package (+ its dependencies) by name or URL (background task).
#[tauri::command]
pub async fn install_mod(app: AppHandle, state: St<'_>, id: String, spec: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    if mods::parse_spec(&spec).is_none() {
        return Err(AppError::msg("Paste the mod's Thunderstore page URL or its Namespace-Name (e.g. ValheimModding-Jotunn)."));
    }
    let st = state.inner().clone();
    let app2 = app.clone();
    background(&app, &id, "Installing a mod", async move {
        let lock = st.op_lock(&inst.id).await;
        let _g = lock.lock().await;
        let r = TaskReporter::new(&app2, "mods", &format!("Installing {}", spec.trim()), Some(&inst.id));
        match mods::install_package(&st.http, &inst, &st.data_dir.join("tmp"), &spec, &r).await {
            Ok(list) => {
                let msg = if list.is_empty() { "Already up to date".to_string() } else { format!("Installed {}", list.join(", ")) };
                r.done(&msg);
                st.history.event(Some(&inst.id), "success", "mods", &msg);
                let _ = app2.emit("mods-changed", &inst.id);
                Ok(())
            }
            Err(e) => {
                r.fail(&e.to_string());
                Err(e)
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn install_mod_file(app: AppHandle, state: St<'_>, id: String, path: PathBuf) -> AppResult<String> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    let name = blocking(move || mods::install_file(&inst, &path)).await?;
    state.history.event(Some(&id), "success", "mods", &format!("Mod installed: {name}"));
    let _ = app.emit("mods-changed", &id);
    Ok(name)
}

#[tauri::command]
pub async fn set_mod_enabled(app: AppHandle, state: St<'_>, id: String, mod_id: String, enabled: bool) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    blocking(move || mods::set_enabled(&inst, &mod_id, enabled)).await?;
    let _ = app.emit("mods-changed", &id);
    Ok(())
}

#[tauri::command]
pub async fn remove_mod(app: AppHandle, state: St<'_>, id: String, mod_id: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    require_stopped(&inst)?;
    let m2 = mod_id.clone();
    blocking(move || mods::remove(&inst, &m2)).await?;
    state.history.event(Some(&id), "info", "mods", &format!("Mod removed: {mod_id}"));
    let _ = app.emit("mods-changed", &id);
    Ok(())
}

#[tauri::command]
pub async fn check_mod_updates(state: St<'_>, id: String) -> AppResult<Vec<mods::ModUpdate>> {
    let inst = state.instance(&id).await?;
    Ok(mods::check_updates(&state.http, &inst).await)
}

#[tauri::command]
pub async fn read_mod_config(state: St<'_>, id: String, name: String) -> AppResult<ConfigFile> {
    let inst = state.instance(&id).await?;
    let path = mods::config_path(&inst, &name)?;
    blocking(move || {
        let meta = std::fs::metadata(&path).ok();
        Ok(ConfigFile {
            exists: meta.is_some(),
            modified: meta.and_then(|m| m.modified().ok()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).timestamp_millis()),
            text: std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
            pending: false,
            path,
        })
    })
    .await
}

/// Mod configs are read by the mods at start – a running server picks the change up on restart.
#[tauri::command]
pub async fn write_mod_config(state: St<'_>, id: String, name: String, text: String) -> AppResult<bool> {
    let inst = state.instance(&id).await?;
    let path = mods::config_path(&inst, &name)?;
    blocking(move || config::write_text(&path, &text)).await?;
    let running = server::running_pids(&inst).1.is_some();
    if running {
        state.with_runtime(&id, |r| r.pending_restart = true).await;
    }
    Ok(running)
}

#[tauri::command]
pub async fn mod_config_history(state: St<'_>, id: String, name: String) -> AppResult<Vec<(String, i64)>> {
    let inst = state.instance(&id).await?;
    history_of(mods::config_path(&inst, &name)?).await
}

#[tauri::command]
pub async fn mods_log(state: St<'_>, id: String, lines: usize) -> AppResult<Vec<String>> {
    let inst = state.instance(&id).await?;
    blocking(move || Ok(mods::log_tail(&inst, lines.clamp(10, 5000)))).await
}

// ------------------------------------------------------------------------------------- history

#[tauri::command]
pub async fn metrics_history(state: St<'_>, id: String, since: i64) -> AppResult<Vec<MetricRow>> {
    Ok(state.history.metrics(&id, since))
}

#[tauri::command]
pub async fn event_history(state: St<'_>, id: Option<String>, limit: u32) -> AppResult<Vec<EventRow>> {
    Ok(state.history.events(id.as_deref(), limit.clamp(1, 2000)))
}

#[tauri::command]
pub async fn player_sessions(state: St<'_>, id: String, limit: u32) -> AppResult<Vec<SessionRow>> {
    Ok(state.history.sessions(&id, limit.clamp(1, 2000)))
}

#[tauri::command]
pub async fn known_players(state: St<'_>, id: String) -> AppResult<Vec<PlayerSummary>> {
    Ok(state.history.players(&id))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRow {
    build_id: String,
    branch: String,
    ts: i64,
}

#[tauri::command]
pub async fn build_history(state: St<'_>, id: String) -> AppResult<Vec<BuildRow>> {
    Ok(state.history.builds(&id).into_iter().map(|(build_id, branch, ts)| BuildRow { build_id, branch, ts }).collect())
}

// ------------------------------------------------------------------------------------- backups

#[tauri::command]
pub async fn create_backup(app: AppHandle, state: St<'_>, id: String, label: String) -> AppResult<BackupInfo> {
    let inst = state.instance(&id).await?;
    state.set_busy(&app, &id, Some("Backing up")).await;
    let res = backup::create(&state, &inst, if label.trim().is_empty() { "Manual backup" } else { label.trim() }, "manual").await;
    state.set_busy(&app, &id, None).await;
    let b = res?;
    state.with_runtime(&id, |r| r.last_backup = b.created_at).await;
    let _ = app.emit("backup-created", &b);
    Ok(b)
}

#[tauri::command]
pub async fn list_backups(state: St<'_>, id: String) -> AppResult<Vec<BackupInfo>> {
    let inst = state.instance(&id).await?;
    let dir = state.backup_dir(&inst).await;
    blocking(move || Ok(backup::list(&dir))).await
}

#[tauri::command]
pub async fn get_backup_dir(state: St<'_>, id: String) -> AppResult<PathBuf> {
    let inst = state.instance(&id).await?;
    Ok(state.backup_dir(&inst).await)
}

#[tauri::command]
pub async fn restore_backup(app: AppHandle, state: St<'_>, id: String, path: PathBuf, include_config: bool) -> AppResult<u32> {
    let inst = state.instance(&id).await?;
    let lock = state.op_lock(&id).await;
    let _g = lock.lock().await;
    require_stopped(&inst)?;
    let dir = state.backup_dir(&inst).await;
    if !proc::is_within(&path, &dir) {
        return Err(AppError::msg("That backup isn't in this server's backup folder."));
    }
    // Safety net: snapshot what's there now.
    if inst.worlds_dir().exists() {
        let _ = backup::create(&state, &inst, "Before restore", "auto").await;
    }
    let i2 = inst.clone();
    let n = blocking(move || backup::restore(&i2, &path, include_config)).await?;
    if include_config {
        let _ = app.emit("access-changed", &id);
    }
    manager_log(&app, &id, format!("Restored {n} files from backup."));
    crate::state::notify_kind(&app, &id, "success", "backup", format!("Restored {n} files from a backup"));
    Ok(n)
}

#[tauri::command]
pub async fn verify_backup(state: St<'_>, id: String, path: PathBuf) -> AppResult<backup::VerifyResult> {
    let inst = state.instance(&id).await?;
    let dir = state.backup_dir(&inst).await;
    if !proc::is_within(&path, &dir) {
        return Err(AppError::msg("That backup isn't in this server's backup folder."));
    }
    blocking(move || backup::verify(&path)).await
}

#[tauri::command]
pub async fn delete_backup(state: St<'_>, id: String, path: PathBuf) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    let dir = state.backup_dir(&inst).await;
    if !proc::is_within(&path, &dir) {
        return Err(AppError::msg("That backup isn't in this server's backup folder."));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

// --------------------------------------------------------------------------------------- packs

#[tauri::command]
pub async fn export_pack(state: St<'_>, id: String, dest: PathBuf, options: backup::PackOptions) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    blocking(move || backup::export_pack(&inst, &dest, &options)).await
}

#[tauri::command]
pub async fn read_pack(path: PathBuf) -> AppResult<PackManifest> {
    blocking(move || backup::read_pack(&path)).await
}

#[tauri::command]
pub async fn import_pack(app: AppHandle, state: St<'_>, path: PathBuf, name: String, install_dir: Option<PathBuf>, install_now: bool) -> AppResult<ServerInstance> {
    let p2 = path.clone();
    let m = blocking(move || backup::read_pack(&p2)).await?;
    let mut inst = m.instance;
    inst.id = uuid::Uuid::new_v4().to_string();
    inst.created_at = now_ms();
    inst.name = if name.trim().is_empty() { inst.name } else { name.trim().to_string() };
    inst.install_dir = match install_dir {
        Some(d) if !d.as_os_str().is_empty() => abs_dir(d)?,
        _ => unique_dir(&state, &inst.name).await,
    };
    inst.launch.data_dir = "./save-data".into();
    assign_free_ports(&state, &mut inst).await;
    let i2 = inst.clone();
    blocking(move || backup::unpack_into(&path, &i2)).await?;
    let inst = add_instance(&state, inst).await?;
    if install_now && !inst.is_installed() {
        let (st, app2, id) = (state.inner().clone(), app.clone(), inst.id.clone());
        background(&app, &inst.id, "Install", async move { server::update_job(&st, &app2, &id, false).await });
    }
    state.emit_status(&app, &inst.id).await;
    Ok(inst)
}

// ------------------------------------------------------------------------------------- network

#[tauri::command]
pub async fn firewall_state(state: St<'_>, id: String) -> AppResult<network::FirewallState> {
    let inst = state.instance(&id).await?;
    Ok(network::firewall_state(&inst).await)
}

#[tauri::command]
pub async fn add_firewall_rules(state: St<'_>, id: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    let out = network::add_firewall_rules(&inst, &state.data_dir.join("tmp")).await?;
    if !out.contains("RULES_OK") {
        return Err(AppError::msg(format!("Windows didn't confirm the rules: {}", out.lines().filter(|l| l.contains("ERROR") || l.contains("Error")).collect::<Vec<_>>().join(" "))));
    }
    Ok(())
}

#[tauri::command]
pub async fn remove_firewall_rules(state: St<'_>, id: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    network::remove_firewall_rules(&inst, &state.data_dir.join("tmp")).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInfo {
    public_ip: Option<String>,
    local: Vec<network::LocalIp>,
    gateway: Option<String>,
}

#[tauri::command]
pub async fn net_info(state: St<'_>) -> AppResult<NetInfo> {
    let (public_ip, gateway) = tokio::join!(network::public_ip(&state.http), network::default_gateway());
    let mut local = blocking(|| Ok(network::local_ips())).await?;
    // Put the adapter on the router's subnet first – that's the address to forward to.
    if let Some(gw) = &gateway {
        let prefix: String = gw.rsplitn(2, '.').nth(1).unwrap_or("").to_string() + ".";
        local.sort_by_key(|l| !l.ip.starts_with(&prefix));
    }
    Ok(NetInfo { public_ip, local, gateway })
}

/// Servers Steam lists at this PC's public IP – what the in-game community list is built from.
#[tauri::command]
pub async fn steam_listing(state: St<'_>, id: String) -> AppResult<Vec<network::SteamListing>> {
    let ip = match state.with_runtime(&id, |r| r.public_ip.clone()).await {
        Some(ip) => ip,
        None => network::public_ip(&state.http).await.ok_or_else(|| AppError::msg("Couldn't find this PC's public IP."))?,
    };
    network::steam_listing(&state.http, &ip).await
}

#[tauri::command]
pub async fn upnp_probe(state: St<'_>) -> AppResult<crate::core::upnp::UpnpInfo> {
    Ok(crate::core::upnp::probe(&state.http).await)
}

#[tauri::command]
pub async fn upnp_map(app: AppHandle, state: St<'_>, id: String) -> AppResult<Vec<String>> {
    let inst = state.instance(&id).await?;
    let lines = crate::core::upnp::map_server(&inst).await?;
    manager_log(&app, &id, format!("UPnP: {}", lines.join(", ")));
    Ok(lines)
}

#[tauri::command]
pub async fn upnp_unmap(state: St<'_>, id: String) -> AppResult<()> {
    let inst = state.instance(&id).await?;
    crate::core::upnp::unmap_server(&inst).await
}

// --------------------------------------------------------------------------------- diagnostics

#[tauri::command]
pub async fn run_diagnostics(state: St<'_>, id: String) -> AppResult<Vec<DiagCheck>> {
    let inst = state.instance(&id).await?;
    Ok(diagnostics::run(&state, &inst).await)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    install: u64,
    saves: u64,
    backups: u64,
    logs: u64,
}

#[tauri::command]
pub async fn disk_usage(state: St<'_>, id: String) -> AppResult<DiskUsage> {
    let inst = state.instance(&id).await?;
    let bk = state.backup_dir(&inst).await;
    blocking(move || {
        let backups = backup::folder_size(&bk);
        let install = backup::folder_size(&inst.install_dir);
        let saves = backup::folder_size(&inst.worlds_dir());
        let inside = proc::is_within(&bk, &inst.install_dir);
        let saves_inside = proc::is_within(&inst.worlds_dir(), &inst.install_dir);
        let mut rest = install;
        if inside {
            rest = rest.saturating_sub(backups);
        }
        if saves_inside {
            rest = rest.saturating_sub(saves);
        }
        Ok(DiskUsage { install: rest, saves, backups, logs: backup::folder_size(&inst.logs_dir()) })
    })
    .await
}
