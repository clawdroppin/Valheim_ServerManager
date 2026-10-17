//! The single background loop: process tracking (incl. servers started outside the manager),
//! state transitions, crash detection & auto-restart, telemetry (live + per-minute history), log
//! tail + player detection, A2S polling, schedules, interval backups, update checks, idle stop and
//! resource-limit restarts.

use super::{a2s, backup, server, steamcmd};
use crate::core::history::MetricRow;
use crate::models::*;
use crate::state::{manager_log, notify, notify_kind, now_ms, AppState, TaskReporter};
use chrono::{Datelike, Local, NaiveTime, TimeZone, Timelike};
use regex::Regex;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use sysinfo::{Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Emitter, Manager};

// Log lines, verified against valheim_server.exe 1.0.20 (Steam build 25909431) – see the tests.
// Every line starts with "MM/dd/yyyy HH:mm:ss: ".
static READY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r": Game server connected\s*$").unwrap());
static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r": Valheim version: (\S+) \(network version (\d+)\)").unwrap());
static STEAMID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r": Server ID (\d+)\s*$").unwrap());
// Crossplay: `Session "Name" registered with join code 748486` and, every few minutes,
// `Session "Name" with join code 748486 and IP 1.2.3.4:2456 is active with 0 player(s)`.
static SESSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Session ".*" (?:registered with join code (\d+)|with join code (\d+) and IP ([0-9.]+):\d+ is active)"#).unwrap());
static SAVED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"World save \(5/5\) done").unwrap());
static SHUTDOWN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r": Game - OnApplicationQuit").unwrap());
static MODDED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:: isModded: True|\[Message:\s+BepInEx\] BepInEx \d)").unwrap());
// Player lines from the binary's strings ("Got connection SteamID ", "Got character ZDOID from ",
// "Closing socket ", "PlayFab socket with remote ID … received local Platform ID …") – not yet
// seen with a real client. A ZDOID of 0:0 is logged when a character dies.
static JOIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Got connection SteamID (7656\d{13})").unwrap());
static PLAYFAB_JOIN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"PlayFab socket with remote ID \S+ received local Platform ID (\S+)").unwrap());
static CHAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Got character ZDOID from (.+?) : (-?\d+):(\d+)\s*$").unwrap());
static LEAVE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Closing socket (7656\d{13})").unwrap());
// Start-up failures. A bad password doesn't stop the server – it hangs half-started (verified), so
// the supervisor ends it.
static FATAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(Error bad password:.*|World load error, saving disabled!.*|Failed to bind socket.*|Failed to initialize platform!|Error saving world! Failed to mount!)\s*$").unwrap()
});

const CRASH_WINDOW_MS: i64 = 15 * 60 * 1000;
const MAX_LINES_PER_TICK: usize = 400;
const QUERY_EVERY_MS: i64 = 10_000;
/// Longest an automatic restart/update waits for players to leave.
const WAIT_EMPTY_MAX_MS: i64 = 60 * 60_000;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
        let mut sup = Supervisor::new();
        sup.boot(&state, &app).await;
        loop {
            let interval = state.settings.read().await.telemetry_interval_ms.clamp(500, 5000);
            let t0 = Instant::now();
            sup.tick(&state, &app).await;
            let spent = t0.elapsed();
            tokio::time::sleep(Duration::from_millis(interval).saturating_sub(spent)).await;
        }
    });
}

/// Samples of one server collected over the current minute (written to manager.db).
#[derive(Default)]
struct MinuteAcc {
    minute: i64,
    n: u32,
    cpu: f64,
    mem: u64,
    read: u64,
    write: u64,
    players: u32,
}

struct Supervisor {
    sys: System,
    nets: Networks,
    last_tick: Instant,
    last_status: HashMap<String, String>,
    over_mem: HashMap<String, u32>,
    hide_until: HashMap<String, i64>,
    /// When a memory/uptime restart was last triggered, per server.
    limit_restart: HashMap<String, i64>,
    /// Ticks since the last full process-list scan.
    since_scan: u32,
    tracked: Vec<sysinfo::Pid>,
    minutes: HashMap<String, MinuteAcc>,
    last_prune: i64,
}

impl Supervisor {
    fn new() -> Self {
        Self {
            sys: System::new(),
            nets: Networks::new_with_refreshed_list(),
            last_tick: Instant::now(),
            last_status: HashMap::new(),
            over_mem: HashMap::new(),
            hide_until: HashMap::new(),
            limit_restart: HashMap::new(),
            since_scan: u32::MAX,
            tracked: vec![],
            minutes: HashMap::new(),
            last_prune: now_ms(),
        }
    }

    async fn boot(&mut self, state: &Arc<AppState>, app: &AppHandle) {
        let instances = state.instances.read().await.clone();
        let check_updates = state.settings.read().await.check_updates_on_launch;
        for inst in &instances {
            let build = steamcmd::local_build(&inst.install_dir);
            let last_bk = backup::list(&state.backup_dir(inst).await).first().map(|b| b.created_at).unwrap_or(0);
            state.history.sessions_close_all(&inst.id, now_ms());
            state
                .with_runtime(&inst.id, |r| {
                    r.installed_build = build;
                    r.last_backup = last_bk;
                    // 0 = check soon after launch; otherwise wait a full interval.
                    r.last_update_check = if check_updates { 0 } else { now_ms() };
                })
                .await;
        }
        // Prime CPU counters so the first sample isn't zero.
        self.sys.refresh_cpu_usage();
        self.refresh_procs();
        tokio::time::sleep(Duration::from_millis(300)).await;
        // Auto-start servers that aren't already running (adopted ones are picked up by tick()).
        for inst in instances.into_iter().filter(|i| i.automation.auto_start && i.is_installed()) {
            let (_, main) = server::running_pids(&inst);
            if main.is_none() {
                let (st, app2) = (state.clone(), app.clone());
                tauri::async_runtime::spawn(async move {
                    manager_log(&app2, &inst.id, "Auto-starting with the manager…");
                    if let Err(e) = server::start(&st, &app2, &inst.id).await {
                        notify(&app2, &inst.id, "error", format!("Auto-start of {} failed: {e}", inst.name));
                    }
                });
            }
        }
    }

    /// Full process discovery (names + exe paths) every 3 ticks; CPU/memory/disk only for the
    /// server processes. Sampling every process on the PC each second would be the main cost.
    fn refresh_procs(&mut self) {
        if self.since_scan >= 3 {
            self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
            self.tracked = self
                .sys
                .processes()
                .iter()
                .filter(|(_, p)| super::proc::is_server_name(&p.name().to_string_lossy()))
                .map(|(pid, _)| *pid)
                .collect();
            self.since_scan = 0;
        }
        self.since_scan += 1;
        if !self.tracked.is_empty() {
            let pids = self.tracked.clone();
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage().with_exe(UpdateKind::OnlyIfNotSet),
            );
            self.tracked.retain(|p| self.sys.process(*p).is_some());
        }
    }

    async fn tick(&mut self, state: &Arc<AppState>, app: &AppHandle) {
        let elapsed = self.last_tick.elapsed().as_secs_f64().max(0.2);
        self.last_tick = Instant::now();
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.refresh_procs();
        self.nets.refresh(true);
        let cores = self.sys.cpus().len().max(1);
        let (rx, tx) = self.nets.iter().fold((0u64, 0u64), |(r, t), (_, d)| (r + d.received(), t + d.transmitted()));
        let system = SystemSample {
            cpu: self.sys.global_cpu_usage(),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            rx_bps: (rx as f64 / elapsed) as u64,
            tx_bps: (tx as f64 / elapsed) as u64,
            cores,
            per_core: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
        };

        let procs: Vec<(u32, String)> = self
            .tracked
            .iter()
            .filter_map(|pid| self.sys.process(*pid).and_then(|p| p.exe()).map(|e| (pid.as_u32(), super::proc::norm(e))))
            .collect();

        let now = now_ms();
        let any_starting = state.runtime.lock().await.values().any(|r| r.state == Some(ServerState::Starting));
        if any_starting {
            self.since_scan = u32::MAX;
        }

        let instances = state.instances.read().await.clone();
        let mut samples = vec![];
        for inst in &instances {
            let base = format!("{}\\", super::proc::norm(&inst.install_dir));
            let pids: Vec<u32> = procs.iter().filter(|(_, e)| e.starts_with(&base)).map(|(p, _)| *p).collect();
            let main = pids.first().copied();

            // Telemetry (sum of the server's processes).
            if !pids.is_empty() {
                let mut s = ProcSample { id: inst.id.clone(), ..Default::default() };
                for pid in &pids {
                    if let Some(p) = self.sys.process(sysinfo::Pid::from_u32(*pid)) {
                        s.cpu += p.cpu_usage() / cores as f32;
                        s.mem += p.memory();
                        s.virt += p.virtual_memory();
                        let du = p.disk_usage();
                        s.read_bps += (du.read_bytes as f64 / elapsed) as u64;
                        s.write_bps += (du.written_bytes as f64 / elapsed) as u64;
                        s.uptime = s.uptime.max(p.run_time());
                    }
                }
                self.resource_limits(state, app, inst, &s).await;
                let players = state.with_runtime(&inst.id, |r| r.query_players.unwrap_or(r.online.len() as u32)).await;
                self.accumulate(state, &inst.id, &s, players, now);
                samples.push(s);
            } else {
                self.over_mem.remove(&inst.id);
                self.flush_minute(state, &inst.id);
            }

            // Tail first: when the process just died, its last log lines say why.
            self.tail_log(state, app, inst).await;
            self.lifecycle(state, app, inst, main, now).await;
            self.query(state, app, inst, now).await;
            self.hide_console(state, inst, &pids, now).await;
            self.automation(state, app, inst, main.is_some(), now).await;

            let st = state.status(inst).await;
            let key = serde_json::to_string(&st).unwrap_or_default();
            if self.last_status.get(&inst.id) != Some(&key) {
                self.last_status.insert(inst.id.clone(), key);
                let _ = app.emit("server-status", st);
            }
        }
        let _ = app.emit("telemetry", TelemetryEvent { ts: now, system, servers: samples });
        if now - self.last_prune > 6 * 3_600_000 {
            self.last_prune = now;
            let days = state.settings.read().await.metrics_retention_days;
            state.history.prune(days);
        }
    }

    /// Average one minute of samples into a manager.db row.
    fn accumulate(&mut self, state: &AppState, id: &str, s: &ProcSample, players: u32, now: i64) {
        let minute = now / 60_000;
        let acc = self.minutes.entry(id.to_string()).or_default();
        if acc.minute != minute && acc.n > 0 {
            let n = acc.n.max(1) as f64;
            state.history.metric(
                id,
                &MetricRow {
                    ts: acc.minute * 60_000,
                    cpu: acc.cpu / n,
                    mem: acc.mem as i64,
                    read_bps: (acc.read as f64 / n) as i64,
                    write_bps: (acc.write as f64 / n) as i64,
                    players: acc.players as i64,
                },
            );
            *acc = MinuteAcc::default();
        }
        acc.minute = minute;
        acc.n += 1;
        acc.cpu += s.cpu as f64;
        acc.mem = acc.mem.max(s.mem);
        acc.read += s.read_bps;
        acc.write += s.write_bps;
        acc.players = acc.players.max(players);
    }

    fn flush_minute(&mut self, state: &AppState, id: &str) {
        if let Some(acc) = self.minutes.remove(id) {
            if acc.n > 0 {
                let n = acc.n as f64;
                state.history.metric(
                    id,
                    &MetricRow { ts: acc.minute * 60_000, cpu: acc.cpu / n, mem: acc.mem as i64, read_bps: (acc.read as f64 / n) as i64, write_bps: (acc.write as f64 / n) as i64, players: acc.players as i64 },
                );
            }
        }
    }

    async fn lifecycle(&mut self, state: &Arc<AppState>, app: &AppHandle, inst: &ServerInstance, main: Option<u32>, now: i64) {
        // A lifecycle operation is running – it owns the state.
        let lock = state.op_lock(&inst.id).await;
        let Ok(_guard) = lock.try_lock() else { return };
        let alive = main.is_some();
        let start_time = main.and_then(|p| self.sys.process(sysinfo::Pid::from_u32(p))).map(|p| p.start_time() as i64 * 1000);

        enum Ev {
            None,
            Adopted,
            Ready,
            Crashed { restart_in: Option<u32>, looped: bool, fatal: Option<String> },
            Stopped,
        }
        let ev = state
            .with_runtime(&inst.id, |r| {
                let cur = r.state.unwrap_or(ServerState::Stopped);
                match (alive, cur) {
                    (true, ServerState::Starting) => {
                        r.pid = main;
                        let slow = r.started_at.map(|t| now - t > 300_000).unwrap_or(false);
                        if r.log_ready || r.query_players.is_some() || slow {
                            r.state = Some(ServerState::Running);
                            r.ready_at = Some(now);
                            return Ev::Ready;
                        }
                        Ev::None
                    }
                    (true, ServerState::Running) => {
                        r.pid = main;
                        Ev::None
                    }
                    (true, _) => {
                        // Started outside the manager or the manager was restarted: adopt it.
                        r.pid = main;
                        r.state = Some(ServerState::Running);
                        r.started_at = start_time.or(Some(now));
                        r.ready_at = Some(now);
                        r.intentional_stop = false;
                        r.restart_at = None;
                        r.log_offset = std::fs::metadata(inst.log_path()).map(|m| m.len()).unwrap_or(0);
                        r.log_sig = log_signature(&inst.log_path());
                        r.log_partial.clear();
                        Ev::Adopted
                    }
                    // Just launched: this tick's process scan may predate the spawn.
                    (false, ServerState::Starting) if r.started_at.map(|t| now - t < 10_000).unwrap_or(false) => Ev::None,
                    (false, ServerState::Starting | ServerState::Running | ServerState::Stopping) => {
                        r.pid = None;
                        r.started_at = None;
                        r.ready_at = None;
                        r.online.clear();
                        r.query_players = None;
                        r.waiting_empty = None;
                        if r.intentional_stop || cur == ServerState::Stopping {
                            r.state = Some(ServerState::Stopped);
                            return Ev::Stopped;
                        }
                        r.state = Some(ServerState::Crashed);
                        r.crash_times.retain(|t| now - t < CRASH_WINDOW_MS);
                        r.crash_times.push(now);
                        // A startup config error repeats on every restart – stop right away.
                        let looped = r.fatal.is_some() || (inst.automation.crash_limit > 0 && r.crash_times.len() as u32 > inst.automation.crash_limit);
                        r.crash_loop = looped;
                        let restart_in = (inst.automation.auto_restart && !looped).then_some(inst.automation.restart_delay_secs);
                        if let Some(d) = restart_in {
                            r.restart_at = Some(now + d as i64 * 1000);
                        }
                        Ev::Crashed { restart_in, looped, fatal: r.fatal.clone() }
                    }
                    _ => Ev::None,
                }
            })
            .await;
        match &ev {
            Ev::Adopted => {
                manager_log(app, &inst.id, format!("Found the server already running (PID {}) – now monitoring it.", main.unwrap_or(0)));
                // Recover version and Steam info from the log written so far (not re-streamed).
                let meta = scan_log_meta(&inst.log_path());
                state
                    .with_runtime(&inst.id, |r| {
                        r.game_version = meta.version;
                        r.steam_id = meta.steam_id;
                        r.public_ip = meta.public_ip;
                        r.log_ready = meta.ready;
                        r.last_save = meta.saved.then_some(now);
                        r.join_code = meta.join_code;
                        r.modded = meta.modded;
                        r.online = meta.online.into_iter().map(|p| (p.key(), p.into_online(now))).collect();
                    })
                    .await;
                let day = super::worlds::active_day(inst);
                state.with_runtime(&inst.id, |r| r.day = day).await;
            }
            Ev::Ready => {
                manager_log(app, &inst.id, format!("Server is online – game port UDP {}, query port UDP {}.", inst.config.port, inst.config.query_port()));
                if inst.config.public && !inst.config.crossplay {
                    spawn_listing_check(state.clone(), app.clone(), inst.id.clone());
                }
                let day = super::worlds::active_day(inst);
                state.with_runtime(&inst.id, |r| r.day = day).await;
                notify_kind(app, &inst.id, "success", "lifecycle", format!("{} is online", inst.name));
            }
            Ev::Crashed { restart_in, looped, fatal } => {
                state.history.sessions_close_all(&inst.id, now);
                let msg = match (*restart_in, *looped) {
                    (_, true) if fatal.is_some() => format!("{} couldn't start ({}) – fix it, then start again.", inst.name, fatal.clone().unwrap_or_default()),
                    (_, true) => format!("{} keeps crashing – auto-restart paused. Check the console and Network & Health.", inst.name),
                    (Some(d), _) => format!("{} stopped unexpectedly – restarting in {d}s", inst.name),
                    (None, _) => format!("{} stopped unexpectedly", inst.name),
                };
                manager_log(app, &inst.id, &msg);
                notify_kind(app, &inst.id, "error", "crash", msg);
            }
            Ev::Stopped => {
                state.history.sessions_close_all(&inst.id, now);
            }
            Ev::None => {}
        }
        if matches!(ev, Ev::Crashed { .. } | Ev::Stopped) {
            state.with_runtime(&inst.id, |r| r.join_code = None).await;
        }

        // Pending crash restart.
        let due = state
            .with_runtime(&inst.id, |r| match r.restart_at {
                Some(t) if t <= now && !alive => {
                    r.restart_at = None;
                    true
                }
                _ => false,
            })
            .await;
        if due {
            let (st, app2, id) = (state.clone(), app.clone(), inst.id.clone());
            tauri::async_runtime::spawn(async move {
                manager_log(&app2, &id, "Auto-restarting after crash…");
                if let Err(e) = server::start(&st, &app2, &id).await {
                    notify(&app2, &id, "error", format!("Auto-restart failed: {e}"));
                }
            });
        }
    }

    async fn hide_console(&mut self, state: &Arc<AppState>, inst: &ServerInstance, pids: &[u32], now: i64) {
        if inst.launch.show_console || pids.is_empty() {
            self.hide_until.remove(&inst.id);
            return;
        }
        // Keep the console hidden for the first minute in case a console host ignores the
        // start-up show state.
        let started = state.with_runtime(&inst.id, |r| r.started_at.unwrap_or(0)).await;
        let until = *self.hide_until.entry(inst.id.clone()).or_insert(started.max(now - 1) + 60_000);
        if now < until {
            let hidden_now = state.with_runtime(&inst.id, |r| r.console_hidden).await;
            if hidden_now {
                super::proc::set_console_visible(pids, false);
            }
        }
    }

    async fn resource_limits(&mut self, state: &Arc<AppState>, app: &AppHandle, inst: &ServerInstance, s: &ProcSample) {
        let a = &inst.automation;
        let running = state.with_runtime(&inst.id, |r| r.state == Some(ServerState::Running)).await;
        if !running {
            return;
        }
        let mut reason = None;
        if a.memory_limit_mb > 0 && s.mem > a.memory_limit_mb as u64 * 1_048_576 {
            let n = self.over_mem.entry(inst.id.clone()).or_insert(0);
            *n += 1;
            if *n >= 30 {
                reason = Some(format!("memory above {} MB for 30 samples", a.memory_limit_mb));
            }
        } else {
            self.over_mem.remove(&inst.id);
        }
        if a.max_uptime_hours > 0 && s.uptime >= a.max_uptime_hours as u64 * 3600 {
            reason = Some(format!("uptime reached {} h", a.max_uptime_hours));
        }
        if let Some(why) = reason {
            self.over_mem.remove(&inst.id);
            // One trigger per 15 minutes: the restart (and its player warning) takes a while, and
            // the uptime stays above the limit until the new process is up.
            let now = now_ms();
            if self.limit_restart.get(&inst.id).is_some_and(|t| now - t < 15 * 60_000) {
                return;
            }
            self.limit_restart.insert(inst.id.clone(), now);
            notify(app, &inst.id, "warn", format!("Restarting {}: {why}", inst.name));
            manager_log(app, &inst.id, format!("Automatic restart: {why}"));
            run_action(state, app, &inst.id, "restart", "Automatic");
        }
    }

    async fn tail_log(&mut self, state: &Arc<AppState>, app: &AppHandle, inst: &ServerInstance) {
        let live = state.with_runtime(&inst.id, |r| r.state.map(|s| s.is_live()).unwrap_or(false)).await;
        if !live {
            return;
        }
        let path = inst.log_path();
        let Ok(meta) = std::fs::metadata(&path) else { return };
        let len = meta.len();
        // A new session's log (the server truncates it at start) – start at its top.
        let sig = log_signature(&path);
        let offset = state
            .with_runtime(&inst.id, |r| {
                if len < r.log_offset {
                    // Shorter than what we read: a fresh file whose header isn't written yet.
                    r.log_offset = 0;
                    r.log_partial.clear();
                    r.log_sig = None;
                }
                if sig.is_some() && r.log_sig != sig {
                    // Already reading this file from the top when the signature first shows up.
                    if r.log_sig.is_some() {
                        r.log_offset = 0;
                        r.log_partial.clear();
                    }
                    r.log_sig = sig.clone();
                }
                r.log_offset
            })
            .await;
        let start = if len < offset { 0 } else { offset };
        if len == start {
            return;
        }
        // Cap one read at 2 MB so a huge backlog can't stall the loop.
        let start = start.max(len.saturating_sub(2 * 1_048_576));
        let mut buf = vec![];
        let ok = std::fs::File::open(&path)
            .and_then(|mut f| {
                f.seek(SeekFrom::Start(start))?;
                f.take(len - start).read_to_end(&mut buf)
            })
            .is_ok();
        if !ok {
            return;
        }
        let text = String::from_utf8_lossy(&buf).into_owned();
        let now = now_ms();
        // (join|leave|death, character name, steam id)
        let mut events: Vec<(&str, String, Option<String>)> = vec![];
        let mut fatal = None;
        let mut kill = false;
        let mut ready = false;
        let mut saved = false;
        let lines = state
            .with_runtime(&inst.id, |r| {
                r.log_offset = len;
                let mut all = std::mem::take(&mut r.log_partial);
                all.push_str(&text);
                let mut lines: Vec<String> = all.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
                r.log_partial = lines.pop().unwrap_or_default();
                for l in &lines {
                    match parse_line(l) {
                        Some(Line::Version(v)) => r.game_version = Some(v),
                        Some(Line::SteamId(v)) => r.steam_id = Some(v),
                        Some(Line::JoinCode(code, ip)) => {
                            r.join_code = Some(code);
                            if ip.is_some() {
                                r.public_ip = ip;
                            }
                        }
                        Some(Line::Modded) => r.modded = true,
                        Some(Line::Ready) => {
                            r.log_ready = true;
                            ready = true;
                        }
                        Some(Line::Saved) => {
                            r.last_save = Some(now);
                            saved = true;
                        }
                        Some(Line::Connected(p)) => {
                            // Not a player yet: wrong passwords and aborted joins connect too.
                            r.online.entry(p.key()).or_insert_with(|| p.into_online(now));
                        }
                        Some(Line::Character(name, dead)) => {
                            if dead {
                                if let Some(o) = r.online.values_mut().find(|o| o.name == name) {
                                    o.deaths += 1;
                                }
                                events.push(("death", name, None));
                                continue;
                            }
                            // The ZDOID line carries no player id: it belongs to the earliest
                            // connection still waiting for its character.
                            let pending = r.online.iter().filter(|(_, o)| o.name.is_empty()).min_by_key(|(_, o)| o.since).map(|(k, _)| k.clone());
                            if let Some(named) = r.online.iter().find(|(_, o)| o.name == name).map(|(k, o)| (k.clone(), o.steam_id.is_none() && o.platform_id.is_none())) {
                                // Known already – a respawn, or a player the Steam query listed first:
                                // then the waiting connection is them, so give the entry its ids.
                                if let (true, Some(p)) = (named.1, pending) {
                                    if let (Some(conn), Some(mut o)) = (r.online.remove(&p), r.online.remove(&named.0)) {
                                        o.steam_id = conn.steam_id;
                                        o.platform_id = conn.platform_id;
                                        r.online.insert(p, o);
                                    }
                                }
                                continue;
                            }
                            match pending.and_then(|k| r.online.get_mut(&k)) {
                                Some(o) => {
                                    o.name = name.clone();
                                    o.since = now;
                                    events.push(("join", name, o.steam_id.clone()));
                                }
                                None => {
                                    r.online.insert(name.clone(), OnlinePlayer { name: name.clone(), since: now, steam_id: None, platform_id: None, deaths: 0 });
                                    events.push(("join", name, None));
                                }
                            }
                        }
                        Some(Line::Left(steam_id)) => {
                            if let Some(gone) = r.online.remove(&steam_id) {
                                // Never got a character: a failed or aborted join – not a session.
                                if !gone.name.is_empty() {
                                    events.push(("leave", gone.name, gone.steam_id));
                                }
                            }
                        }
                        Some(Line::Shutdown) => {
                            // A quit the manager didn't ask for (Ctrl+C in a visible console): the
                            // server saves and exits cleanly – not a crash.
                            r.intentional_stop = true;
                        }
                        Some(Line::Fatal(m)) => {
                            if !r.log_ready {
                                r.fatal = Some(m.clone());
                                kill = true;
                            }
                            if fatal.is_none() {
                                fatal = Some(m);
                            }
                        }
                        None => {}
                    }
                }
                lines
            })
            .await;
        if lines.is_empty() {
            return;
        }
        let shown: Vec<String> = if lines.len() > MAX_LINES_PER_TICK {
            let mut v = vec![format!("… {} earlier lines skipped (open the log file for everything)", lines.len() - MAX_LINES_PER_TICK)];
            v.extend(lines[lines.len() - MAX_LINES_PER_TICK..].iter().cloned());
            v
        } else {
            lines
        };
        let _ = app.emit("log-lines", LogBatch { id: inst.id.clone(), source: "server".into(), lines: shown, ts: now });
        for (kind, name, steam_id) in events {
            match kind {
                "join" => {
                    // The session starts with the character, not the connection.
                    state.history.session_start(&inst.id, &name, steam_id.as_deref(), now);
                    notify_kind(app, &inst.id, "info", "player", format!("{name} joined {}", inst.name));
                }
                "leave" => {
                    state.history.session_end(&inst.id, &name, steam_id.as_deref(), now);
                    notify_kind(app, &inst.id, "info", "player", format!("{name} left {}", inst.name));
                }
                _ => manager_log(app, &inst.id, format!("{name} died.")),
            }
            let _ = app.emit("player-event", PlayerEvent { id: inst.id.clone(), name, kind: kind.into(), ts: now });
        }
        if let Some(m) = fatal {
            notify_kind(app, &inst.id, "error", "crash", format!("Server error: {m}"));
        }
        if kill {
            // The server doesn't exit on these (verified for a bad password) – end it so the crash
            // handling reports why and doesn't loop.
            let (pids, _) = server::running_pids(inst);
            manager_log(app, &inst.id, "The server can't run with this error – stopping it.");
            for p in pids {
                let _ = super::proc::kill_tree(p).await;
            }
        }
        if saved || ready {
            let day = super::worlds::active_day(inst);
            state.with_runtime(&inst.id, |r| r.day = day).await;
        }
        if ready {
            let _ = app.emit("server-ready", &inst.id);
        }
    }

    /// A2S every 10 s (Valheim answers on port + 1 – verified, keywords `g=1.0.20,n=41,m=`): the
    /// player count the server itself reports, and names to reconcile the log-based list with
    /// (players who joined before the manager opened).
    async fn query(&mut self, state: &Arc<AppState>, app: &AppHandle, inst: &ServerInstance, now: i64) {
        let due = state
            .with_runtime(&inst.id, |r| {
                let live = matches!(r.state, Some(ServerState::Running) | Some(ServerState::Starting));
                let due = live && now - r.last_query >= QUERY_EVERY_MS;
                if due {
                    r.last_query = now;
                }
                due
            })
            .await;
        if !due {
            return;
        }
        let addr = a2s::local_addr("", inst.config.query_port());
        let Some(info) = a2s::info(addr, Duration::from_millis(1500)).await else { return };
        let names = if info.players > 0 { a2s::players(addr, Duration::from_millis(1500)).await } else { Some(vec![]) };
        let mut joined: Vec<(String, Option<String>)> = vec![];
        let mut left: Vec<(String, Option<String>)> = vec![];
        // `g=1.0.20,n=41,m=…`: the game version, for servers adopted after their log was rotated.
        let kw_version = info.keywords.split(',').find_map(|k| k.strip_prefix("g=")).map(|v| v.to_string()).filter(|v| !v.is_empty());
        state
            .with_runtime(&inst.id, |r| {
                r.query_players = Some(info.players);
                if r.game_version.is_none() {
                    r.game_version = kw_version;
                }
                // Only trust A2S names when every slot has one; matched by character name.
                if let Some(list) = names.filter(|l| l.len() as u32 == info.players && l.iter().all(|p| !p.name.is_empty())) {
                    for p in &list {
                        if r.online.values().any(|o| o.name == p.name) {
                            continue;
                        }
                        let since = now - (p.duration_secs.max(0.0) * 1000.0) as i64;
                        // Only one connection waiting for a character: the name is theirs.
                        let pending: Vec<String> = r.online.iter().filter(|(_, o)| o.name.is_empty()).map(|(k, _)| k.clone()).collect();
                        if let [k] = pending.as_slice() {
                            if let Some(o) = r.online.get_mut(k) {
                                o.name = p.name.clone();
                                joined.push((p.name.clone(), o.steam_id.clone()));
                                continue;
                            }
                        }
                        r.online.insert(p.name.clone(), OnlinePlayer { name: p.name.clone(), since, steam_id: None, platform_id: None, deaths: 0 });
                        joined.push((p.name.clone(), None));
                    }
                    let gone: Vec<String> = r.online.iter().filter(|(_, o)| !o.name.is_empty() && !list.iter().any(|p| p.name == o.name)).map(|(k, _)| k.clone()).collect();
                    for k in gone {
                        if let Some(o) = r.online.remove(&k) {
                            left.push((o.name, o.steam_id));
                        }
                    }
                } else if info.players == 0 && !r.online.is_empty() {
                    left.extend(std::mem::take(&mut r.online).into_values().filter(|o| !o.name.is_empty()).map(|o| (o.name, o.steam_id)));
                }
            })
            .await;
        for (n, sid) in joined {
            state.history.session_start(&inst.id, &n, sid.as_deref(), now);
            let _ = app.emit("player-event", PlayerEvent { id: inst.id.clone(), name: n, kind: "join".into(), ts: now });
        }
        for (n, sid) in left {
            state.history.session_end(&inst.id, &n, sid.as_deref(), now);
            let _ = app.emit("player-event", PlayerEvent { id: inst.id.clone(), name: n, kind: "leave".into(), ts: now });
        }
    }

    async fn automation(&mut self, state: &Arc<AppState>, app: &AppHandle, inst: &ServerInstance, alive: bool, now: i64) {
        let a = &inst.automation;
        let local = Local::now();
        let online = state.with_runtime(&inst.id, |r| r.query_players.unwrap_or(r.online.len() as u32)).await;

        // Schedules: fire on the matching minute, compute the next one for the UI.
        let mut next: Option<NextSchedule> = None;
        let mut fire = vec![];
        for (idx, e) in a.schedules.iter().enumerate().filter(|(_, e)| e.enabled) {
            let Ok(t) = NaiveTime::parse_from_str(e.time.trim(), "%H:%M") else { continue };
            let day_ok = |wd: u32| e.days.is_empty() || e.days.contains(&(wd as u8));
            if day_ok(local.weekday().num_days_from_sunday()) && local.hour() == t.hour() && local.minute() == t.minute() {
                let key = format!("{}-{idx}-{}-{}", local.format("%Y%m%d"), e.time, e.action);
                let fresh = state.with_runtime(&inst.id, |r| r.fired_schedule_keys.insert(key)).await;
                if fresh {
                    fire.push(e.action.clone());
                }
            }
            for d in 0..8 {
                let date = local.date_naive() + chrono::Days::new(d);
                let Some(dt) = Local.from_local_datetime(&date.and_time(t)).single() else { continue };
                if dt <= local || !day_ok(dt.weekday().num_days_from_sunday()) {
                    continue;
                }
                let at = dt.timestamp_millis();
                if next.as_ref().map(|n| at < n.at).unwrap_or(true) {
                    next = Some(NextSchedule { at, action: e.action.clone() });
                }
                break;
            }
        }
        state
            .with_runtime(&inst.id, |r| {
                r.next_schedule = next;
                if r.fired_schedule_keys.len() > 200 {
                    r.fired_schedule_keys.clear();
                }
            })
            .await;
        for action in fire {
            // Disruptive actions wait for an empty server when asked to.
            if alive && online > 0 && a.wait_for_empty && matches!(action.as_str(), "restart" | "update" | "stop") {
                state.with_runtime(&inst.id, |r| r.waiting_empty = Some((action.clone(), now))).await;
                manager_log(app, &inst.id, format!("Scheduled {action} is waiting for {online} player(s) to leave (at most 60 minutes)."));
                continue;
            }
            manager_log(app, &inst.id, format!("Scheduled {action}"));
            run_action(state, app, &inst.id, &action, "Scheduled");
        }

        // A deferred action whose wait is over.
        let waiting = state
            .with_runtime(&inst.id, |r| match &r.waiting_empty {
                // The server went down meanwhile: only an update still makes sense.
                Some((act, _)) if !alive => {
                    let act = (act == "update").then(|| act.clone());
                    r.waiting_empty = None;
                    act
                }
                Some((act, since)) if online == 0 || now - since > WAIT_EMPTY_MAX_MS => {
                    let act = act.clone();
                    r.waiting_empty = None;
                    Some(act)
                }
                _ => None,
            })
            .await;
        if let Some(action) = waiting {
            manager_log(app, &inst.id, format!("Running the deferred {action} now."));
            run_action(state, app, &inst.id, &action, "Deferred");
        }

        // Interval backups while running.
        if alive && a.backup_interval_mins > 0 {
            let due = state
                .with_runtime(&inst.id, |r| {
                    if r.last_backup == 0 {
                        r.last_backup = now;
                    }
                    let due = r.state == Some(ServerState::Running) && r.busy.is_none() && now - r.last_backup >= a.backup_interval_mins as i64 * 60_000;
                    if due {
                        r.last_backup = now; // claim it so the next tick doesn't double-fire
                    }
                    due
                })
                .await;
            if due {
                let (st, app2, id) = (state.clone(), app.clone(), inst.id.clone());
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = run_backup(&st, &app2, &id, "Interval", "auto").await {
                        notify(&app2, &id, "warn", format!("Automatic backup failed: {e}"));
                    }
                });
            }
        }

        // Idle stop.
        if alive && a.idle_stop_mins > 0 {
            let stop = state
                .with_runtime(&inst.id, |r| {
                    if r.state != Some(ServerState::Running) || online > 0 {
                        r.empty_since = None;
                        return false;
                    }
                    let since = *r.empty_since.get_or_insert(now);
                    if now - since >= a.idle_stop_mins as i64 * 60_000 {
                        r.empty_since = None;
                        return true;
                    }
                    false
                })
                .await;
            if stop {
                notify_kind(app, &inst.id, "info", "lifecycle", format!("{} was empty for {} minutes – stopping it", inst.name, a.idle_stop_mins));
                run_action(state, app, &inst.id, "stop", "Idle");
            }
        }

        // Update checks.
        if a.update_check_mins > 0 && inst.is_installed() {
            let due = state
                .with_runtime(&inst.id, |r| {
                    let due = now - r.last_update_check >= a.update_check_mins as i64 * 60_000 && r.busy.is_none();
                    if due {
                        r.last_update_check = now;
                    }
                    due
                })
                .await;
            if due {
                let (st, app2, id) = (state.clone(), app.clone(), inst.id.clone());
                tauri::async_runtime::spawn(async move {
                    let _ = check_update(&st, &app2, &id, true).await;
                });
            }
        }
    }
}

/// Three minutes after a public Steam-backend server came online, ask Steam whether it lists it –
/// the Community tab is built from that list, so "not listed" means nobody can find it by name.
fn spawn_listing_check(state: Arc<AppState>, app: AppHandle, id: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(180)).await;
        let Ok(inst) = state.instance(&id).await else { return };
        let (running, ip) = state.with_runtime(&id, |r| (r.state == Some(ServerState::Running), r.public_ip.clone())).await;
        if !running {
            return;
        }
        let Some(ip) = (match ip {
            Some(ip) => Some(ip),
            None => super::network::public_ip(&state.http).await,
        }) else {
            return;
        };
        match super::network::steam_listing(&state.http, &ip).await {
            Ok(list) if list.iter().any(|s| s.gameport == inst.config.port || s.gameport == inst.config.query_port()) => {
                manager_log(&app, &id, format!("Steam lists the server at {ip} – players can find it under Community."));
            }
            Ok(_) => notify(
                &app,
                &id,
                "warn",
                format!("Steam doesn't list {} at {ip} yet – check the firewall and port forwarding (Network & Health), or turn on Crossplay.", inst.name),
            ),
            Err(_) => {}
        }
    });
}

fn run_action(state: &Arc<AppState>, app: &AppHandle, id: &str, action: &str, why: &'static str) {
    let (st, app2, id, action) = (state.clone(), app.clone(), id.to_string(), action.to_string());
    tauri::async_runtime::spawn(async move {
        let res = match action.as_str() {
            "restart" => server::restart(&st, &app2, &id, false).await,
            "update" => server::update_job(&st, &app2, &id, false).await,
            "stop" => server::stop(&st, &app2, &id, false).await,
            "start" => server::start(&st, &app2, &id).await,
            "backup" => run_backup(&st, &app2, &id, "Scheduled", "auto").await,
            _ => Ok(()),
        };
        if let Err(e) = res {
            notify(&app2, &id, "error", format!("{why} {action} failed: {e}"));
        }
    });
}

/// A player as the log identifies them.
#[derive(Debug, PartialEq, Clone)]
struct LogPlayer {
    steam_id: Option<String>,
    /// `Steam_7656…` / `Xbox_…`
    platform_id: Option<String>,
    name: String,
}

impl LogPlayer {
    fn key(&self) -> String {
        self.steam_id.clone().or_else(|| self.platform_id.clone()).unwrap_or_else(|| self.name.clone())
    }
    fn into_online(self, since: i64) -> OnlinePlayer {
        OnlinePlayer { name: self.name, since, steam_id: self.steam_id, platform_id: self.platform_id, deaths: 0 }
    }
}

#[derive(Debug, PartialEq)]
enum Line {
    Version(String),
    SteamId(String),
    /// Crossplay join code and, from the periodic session line, the public IP.
    JoinCode(String, Option<String>),
    Modded,
    Ready,
    Saved,
    Connected(LogPlayer),
    /// Character name; true when the character died (ZDOID 0:0).
    Character(String, bool),
    /// SteamID64 of a closed connection.
    Left(String),
    Shutdown,
    Fatal(String),
}

fn parse_line(l: &str) -> Option<Line> {
    if l.len() < 12 {
        return None;
    }
    if let Some(c) = CHAR_RE.captures(l) {
        let dead = &c[2] == "0" && &c[3] == "0";
        return Some(Line::Character(c[1].trim().to_string(), dead));
    }
    if let Some(c) = JOIN_RE.captures(l) {
        let sid = c[1].to_string();
        return Some(Line::Connected(LogPlayer { platform_id: Some(format!("Steam_{sid}")), steam_id: Some(sid), name: String::new() }));
    }
    if let Some(c) = PLAYFAB_JOIN_RE.captures(l) {
        let pid = c[1].to_string();
        let steam_id = super::config::steam_part(&pid);
        return Some(Line::Connected(LogPlayer { steam_id, platform_id: Some(pid), name: String::new() }));
    }
    if let Some(c) = LEAVE_RE.captures(l) {
        return Some(Line::Left(c[1].to_string()));
    }
    if SAVED_RE.is_match(l) {
        return Some(Line::Saved);
    }
    if let Some(c) = SESSION_RE.captures(l) {
        let code = c.get(1).or(c.get(2)).map(|m| m.as_str().to_string())?;
        return Some(Line::JoinCode(code, c.get(3).map(|m| m.as_str().to_string())));
    }
    if READY_RE.is_match(l) {
        return Some(Line::Ready);
    }
    if let Some(c) = VERSION_RE.captures(l) {
        return Some(Line::Version(c[1].to_string()));
    }
    if let Some(c) = STEAMID_RE.captures(l) {
        return Some(Line::SteamId(c[1].to_string()));
    }
    if SHUTDOWN_RE.is_match(l) {
        return Some(Line::Shutdown);
    }
    if MODDED_RE.is_match(l) {
        return Some(Line::Modded);
    }
    if let Some(c) = FATAL_RE.captures(l) {
        return Some(Line::Fatal(c[1].trim().to_string()));
    }
    None
}

/// What tells one log session from the next: the timestamped `Setting -savedir to:` /
/// `Valheim version:` line near the top (the Unity banner before it is identical every time).
pub fn log_signature(path: &std::path::Path) -> Option<String> {
    let mut buf = vec![0u8; 24 * 1024];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).ok()?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.lines().find(|l| l.contains(": Setting -savedir to:") || l.contains(": Valheim version:")).map(|l| l.trim().to_string())
}

#[derive(Default)]
struct LogMeta {
    version: Option<String>,
    steam_id: Option<String>,
    public_ip: Option<String>,
    join_code: Option<String>,
    modded: bool,
    ready: bool,
    saved: bool,
    online: Vec<LogPlayer>,
}

/// Version, Steam info and who is online, recovered from a log written so far.
fn scan_log_meta(path: &std::path::Path) -> LogMeta {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut m = LogMeta::default();
    let mut online: Vec<LogPlayer> = vec![];
    for l in text.lines() {
        match parse_line(l) {
            Some(Line::Version(v)) => m.version = Some(v),
            Some(Line::SteamId(v)) => m.steam_id = Some(v),
            Some(Line::JoinCode(c, ip)) => {
                m.join_code = Some(c);
                if ip.is_some() {
                    m.public_ip = ip;
                }
            }
            Some(Line::Modded) => m.modded = true,
            Some(Line::Ready) => m.ready = true,
            Some(Line::Saved) => m.saved = true,
            Some(Line::Connected(p)) => {
                online.retain(|x| x.key() != p.key());
                online.push(p);
            }
            Some(Line::Character(name, false)) => {
                if !online.iter().any(|x| x.name == name) {
                    match online.iter_mut().find(|x| x.name.is_empty()) {
                        Some(p) => p.name = name,
                        None => online.push(LogPlayer { steam_id: None, platform_id: None, name }),
                    }
                }
            }
            Some(Line::Left(sid)) => online.retain(|x| x.steam_id.as_deref() != Some(sid.as_str())),
            _ => {}
        }
    }
    // Connections that never got a character aren't players.
    m.online = online.into_iter().filter(|p| !p.name.is_empty()).collect();
    m
}

pub async fn run_backup(state: &Arc<AppState>, app: &AppHandle, id: &str, label: &str, kind: &str) -> crate::error::AppResult<()> {
    let inst = state.instance(id).await?;
    state.set_busy(app, id, Some("Backing up")).await;
    let res = backup::create(state, &inst, label, kind).await;
    state.set_busy(app, id, None).await;
    let b = res?;
    state.with_runtime(id, |r| r.last_backup = b.created_at).await;
    let _ = app.emit("backup-created", &b);
    Ok(())
}

/// Look up the live build for the server's branch (cached 10 min across servers) and, when the
/// profile asks for it, apply the update automatically. Returns true when an update is available.
pub async fn check_update(state: &Arc<AppState>, app: &AppHandle, id: &str, background: bool) -> crate::error::AppResult<bool> {
    let inst = state.instance(id).await?;
    let branch = if inst.branch.trim().is_empty() { "public".to_string() } else { inst.branch.trim().to_string() };
    let cached = state.latest_build_cache.lock().await.get(&branch).cloned().filter(|(t, _)| now_ms() - t < 10 * 60_000);
    let latest = match cached {
        Some((_, b)) => b,
        None => {
            let mut r = TaskReporter::new(app, "update-check", "Checking for server updates", Some(id));
            if background {
                r = r.quiet();
            }
            let res = match steamcmd::latest_build_web(&state.http, &branch).await {
                Some(b) => Ok(Some(b)),
                None => {
                    let steam = state.steamcmd_dir().await;
                    let _l = state.steam_lock.lock().await;
                    steamcmd::latest_build(&state.http, &steam, &branch, &r).await
                }
            };
            let b = match res {
                Ok(b) => b,
                Err(e) => {
                    if !background {
                        r.fail(&e.to_string());
                    }
                    return Err(e);
                }
            };
            if !background {
                r.done(&format!("Live build: {}", b.clone().unwrap_or_else(|| "unknown".into())));
            }
            state.latest_build_cache.lock().await.insert(branch, (now_ms(), b.clone()));
            b
        }
    };
    let installed = steamcmd::local_build(&inst.install_dir);
    let (was, online, alive) = state
        .with_runtime(id, |r| {
            let was = r.latest_build.clone();
            r.latest_build = latest.clone();
            r.installed_build = installed.clone();
            (was, r.query_players.unwrap_or(r.online.len() as u32), r.pid.is_some())
        })
        .await;
    state.emit_status(app, id).await;
    let available = matches!((&installed, &latest), (Some(i), Some(l)) if i != l);
    if available && background {
        if inst.automation.auto_update {
            if alive && online > 0 && inst.automation.wait_for_empty {
                let fresh = state
                    .with_runtime(id, |r| {
                        let fresh = r.waiting_empty.is_none();
                        r.waiting_empty.get_or_insert(("update".into(), now_ms()));
                        fresh
                    })
                    .await;
                if fresh {
                    notify_kind(app, id, "info", "update", format!("Update found for {} – it installs once the {online} player(s) online have left", inst.name));
                }
            } else {
                notify_kind(app, id, "info", "update", format!("Update found for {} – applying it now", inst.name));
                server::update_job(state, app, id, false).await?;
            }
        } else if was != latest {
            notify_kind(app, id, "info", "update", format!("A server update is available for {} (build {})", inst.name, latest.clone().unwrap_or_default()));
        }
    }
    Ok(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_server_log_lines() {
        // Captured from real runs (valheim_server.exe 1.0.20, Steam build 25909431).
        assert_eq!(parse_line("10/16/2026 08:10:25: Valheim version: 1.0.20 (network version 41)"), Some(Line::Version("1.0.20".into())));
        assert_eq!(parse_line("10/16/2026 08:10:25: Server ID 90071992547409920"), Some(Line::SteamId("90071992547409920".into())));
        assert_eq!(parse_line("10/16/2026 08:10:32: Game server connected"), Some(Line::Ready));
        assert_eq!(parse_line("10/16/2026 08:12:08: World save (5/5) done. Total time [35ms]"), Some(Line::Saved));
        assert_eq!(parse_line("10/16/2026 08:12:08: Game - OnApplicationQuit"), Some(Line::Shutdown));
        assert_eq!(parse_line("10/16/2026 08:17:01: Session \"VSM Test\" registered with join code 748486"), Some(Line::JoinCode("748486".into(), None)));
        assert_eq!(
            parse_line("10/16/2026 08:17:02: Session \"VSM Test\" with join code 748486 and IP 32.219.170.177:2456 is active with 0 player(s)"),
            Some(Line::JoinCode("748486".into(), Some("32.219.170.177".into())))
        );
        assert_eq!(parse_line("10/16/2026 08:16:59: New session server \"VSM Test\" that has join code , now 0 player(s)"), None);
        assert_eq!(parse_line("10/16/2026 08:16:13: Error bad password:The password is too short"), Some(Line::Fatal("Error bad password:The password is too short".into())));
        assert_eq!(parse_line("10/16/2026 08:10:26: isModded: False"), None);
        assert_eq!(parse_line("10/16/2026 08:10:26: isModded: True"), Some(Line::Modded));
        assert_eq!(parse_line("10/16/2026 08:12:08: Game server disconnected"), None);
        assert_eq!(parse_line("10/16/2026 08:12:08: World save (4/5) FWL writing done [1ms]"), None);
        assert_eq!(parse_line("10/16/2026 08:12:08: Closing socket"), None);
        // From the binary's format strings.
        assert_eq!(
            parse_line("10/16/2026 09:00:00: Got connection SteamID 76561198000000001"),
            Some(Line::Connected(LogPlayer { steam_id: Some("76561198000000001".into()), platform_id: Some("Steam_76561198000000001".into()), name: "".into() }))
        );
        assert_eq!(parse_line("10/16/2026 09:00:20: Got character ZDOID from Bjørn Ironside : -1934829:1"), Some(Line::Character("Bjørn Ironside".into(), false)));
        assert_eq!(parse_line("10/16/2026 09:30:00: Got character ZDOID from Bjørn Ironside : 0:0"), Some(Line::Character("Bjørn Ironside".into(), true)));
        assert_eq!(parse_line("10/16/2026 09:40:00: Closing socket 76561198000000001"), Some(Line::Left("76561198000000001".into())));
        assert_eq!(
            parse_line("10/16/2026 09:00:00: PlayFab socket with remote ID playfab/ABCDEF received local Platform ID Xbox_2535412345678901"),
            Some(Line::Connected(LogPlayer { steam_id: None, platform_id: Some("Xbox_2535412345678901".into()), name: "".into() }))
        );
    }

    #[test]
    fn log_meta_tracks_who_is_still_online() {
        let dir = std::env::temp_dir().join(format!("vhsm-meta-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("valheim_server.log");
        std::fs::write(
            &p,
            "Mono path[0] = 'x'\n\
             10/16/2026 08:10:25: Setting -savedir to: O:\\srv\\save-data\n\
             10/16/2026 08:10:25: Valheim version: 1.0.20 (network version 41)\n\
             10/16/2026 08:10:32: Game server connected\n\
             10/16/2026 08:17:01: Session \"VSM Test\" registered with join code 748486\n\
             10/16/2026 09:00:00: Got connection SteamID 76561198000000001\n\
             10/16/2026 09:00:20: Got character ZDOID from Bjorn : -1934829:1\n\
             10/16/2026 09:01:00: Got connection SteamID 76561198000000003\n\
             10/16/2026 09:01:30: Got character ZDOID from Freya : 51:1\n\
             10/16/2026 09:02:00: Got connection SteamID 76561198000000005\n\
             10/16/2026 09:02:01: Closing socket 76561198000000005\n\
             10/16/2026 09:30:00: Closing socket 76561198000000001\n",
        )
        .unwrap();
        let m = scan_log_meta(&p);
        assert!(m.ready);
        assert_eq!(m.version.as_deref(), Some("1.0.20"));
        assert_eq!(m.join_code.as_deref(), Some("748486"));
        assert_eq!(m.online.len(), 1);
        assert_eq!(m.online[0].name, "Freya");
        assert_eq!(m.online[0].steam_id.as_deref(), Some("76561198000000003"));
        assert_eq!(log_signature(&p).as_deref(), Some("10/16/2026 08:10:25: Setting -savedir to: O:\\srv\\save-data"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
