//! Persisted and event types. Everything is camelCase on the wire and mirrored in `src/lib/types.ts`.
//!
//! The Valheim dedicated server (verified against Steam build 25909431, Valheim 1.0.20) is
//! configured entirely on its command line (`-name -port -world -password -public -crossplay
//! -savedir -saveinterval -backups … -preset -modifier -setkey`). Everything it writes lives in the
//! `-savedir` folder: `worlds_local\<World>\` (one folder per world: `_main.<N>.fwl2/.db2/.chunks/.ok`
//! plus chunk files) and `adminlist.txt` / `bannedlist.txt` / `permittedlist.txt` (one
//! `Platform_UserId` per line). Live player info comes from the log and the Steam A2S query on
//! port + 1.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// "Valheim Dedicated Server" – free Steam tool, anonymous SteamCMD login.
pub const SERVER_APP_ID: u32 = 896660;
/// The game client (Steam master list `appid`, `SteamAppId` for the server, its local saves).
pub const GAME_APP_ID: u32 = 892970;
pub const SERVER_EXE: &str = "valheim_server.exe";
pub const ADMIN_FILE: &str = "adminlist.txt";
pub const BAN_FILE: &str = "bannedlist.txt";
pub const PERMIT_FILE: &str = "permittedlist.txt";
pub const LOG_FILE: &str = "valheim_server.log";
/// The folder under `-savedir` the server keeps worlds in.
pub const WORLDS_DIR: &str = "worlds_local";
pub const DEFAULT_GAME_PORT: u16 = 2456;
/// One in-game day is 1800 seconds of world time (`EnvMan.m_dayLengthSec`).
pub const DAY_SECS: f64 = 1800.0;

/// The world-modifier presets `-preset` accepts (manual + binary strings).
pub const PRESETS: &[&str] = &["normal", "casual", "easy", "hard", "hardcore", "immersive", "hammer"];
/// `-modifier <key> <value>` and the values each accepts ("default" = leave it at the preset's value).
pub const MODIFIERS: &[(&str, &[&str])] = &[
    ("combat", &["veryeasy", "easy", "hard", "veryhard"]),
    ("deathpenalty", &["casual", "veryeasy", "easy", "hard", "hardcore"]),
    ("resources", &["muchless", "less", "more", "muchmore", "most"]),
    ("raids", &["none", "muchless", "less", "more", "muchmore"]),
    ("portals", &["casual", "hard", "veryhard"]),
];
/// `-setkey` checkbox keys.
pub const SET_KEYS: &[&str] = &["nobuildcost", "playerevents", "passivemobs", "nomap"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub steamcmd_dir: Option<PathBuf>,
    pub default_install_root: Option<PathBuf>,
    pub backup_dir: Option<PathBuf>,
    pub last_data_dir: Option<PathBuf>,
    /// Supervisor sample interval for live telemetry.
    pub telemetry_interval_ms: u64,
    pub confirm_destructive: bool,
    pub reduce_motion: bool,
    pub check_updates_on_launch: bool,
    pub accent: String,
    /// Closing the window keeps the manager (and its automation) running in the tray.
    pub close_to_tray: bool,
    /// Windows toast notifications for crashes, updates and players.
    pub desktop_notifications: bool,
    /// Masks IPs, passwords, join codes and player ids in the UI (for streaming / screenshots).
    pub streamer_mode: bool,
    /// Discord/Slack-compatible webhook for server events. Empty = off. Stored encrypted.
    pub webhook_url: String,
    /// Event levels sent to the webhook: success | error | warn | info
    pub webhook_levels: Vec<String>,
    /// Days of per-minute metrics kept in manager.db.
    pub metrics_retention_days: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            steamcmd_dir: None,
            default_install_root: None,
            backup_dir: None,
            last_data_dir: None,
            telemetry_interval_ms: 1000,
            confirm_destructive: true,
            reduce_motion: false,
            check_updates_on_launch: true,
            accent: "fjord".into(),
            close_to_tray: true,
            desktop_notifications: true,
            streamer_mode: false,
            webhook_url: String::new(),
            webhook_levels: vec!["success".into(), "error".into()],
            metrics_retention_days: 30,
        }
    }
}

/// Everything the server's command line carries. The server has no settings file of its own.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    /// `-name`: shown in the server browser.
    pub name: String,
    /// `-port`: the server uses this UDP port and the next one (Steam query).
    pub port: u16,
    /// `-world`: the world under `worlds_local` the server loads (created on first start).
    pub world: String,
    /// `-password`: at least 5 characters and not part of the server name; empty = open server.
    pub password: String,
    /// `-public 1|0`: listed in the in-game server browser.
    pub public: bool,
    /// `-crossplay`: PlayFab backend (Xbox / Game Pass players, join codes, no port forwarding).
    pub crossplay: bool,
    /// `-instanceid`: unique PlayFab id when several crossplay servers share a port and MAC.
    pub instance_id: String,
    /// `-saveinterval` in seconds (default 1800).
    pub save_interval: u32,
    /// `-backups` / `-backupshort` / `-backuplong`: the server's own world backups.
    pub backups: u32,
    pub backup_short: u32,
    pub backup_long: u32,
    /// `-preset`: one of `PRESETS`; empty = leave the world's stored modifiers alone.
    pub preset: String,
    /// `-modifier <key> <value>`; missing/empty = the preset's value.
    pub modifiers: BTreeMap<String, String>,
    /// `-setkey <key>` checkboxes.
    pub keys: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "Valheim Server".into(),
            port: DEFAULT_GAME_PORT,
            world: "Dedicated".into(),
            password: String::new(),
            public: true,
            crossplay: false,
            instance_id: String::new(),
            save_interval: 1800,
            backups: 4,
            backup_short: 7200,
            backup_long: 43200,
            preset: String::new(),
            modifiers: BTreeMap::new(),
            keys: vec![],
        }
    }
}

impl ServerConfig {
    pub fn query_port(&self) -> u16 {
        self.port.saturating_add(1)
    }
    /// True when the profile sets any world modifier (they are written into the world at start).
    pub fn has_modifiers(&self) -> bool {
        !self.preset.trim().is_empty() || self.modifiers.values().any(|v| !v.trim().is_empty() && v != "default") || !self.keys.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchSettings {
    /// `-savedir`: worlds and the permission lists. Relative = inside the install folder.
    pub data_dir: String,
    /// Ask the router to forward the game and query ports over UPnP on every start.
    pub upnp: bool,
    /// Keep the server's console window visible (hidden by default).
    pub show_console: bool,
    /// normal | belowNormal | aboveNormal | high | idle
    pub priority: String,
    /// CPU cores the server may use. Empty = all cores.
    pub affinity: Vec<u32>,
    /// Free-form extra arguments appended to the command line.
    pub extra_args: String,
    /// Load BepInEx mods when BepInEx is installed (off = start vanilla without uninstalling).
    pub mods_enabled: bool,
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self { data_dir: "./save-data".into(), upnp: false, show_console: false, priority: "normal".into(), affinity: vec![], extra_args: String::new(), mods_enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ScheduleEntry {
    pub enabled: bool,
    /// Local time, "HH:MM".
    pub time: String,
    /// 0 = Sunday … 6 = Saturday. Empty = every day.
    pub days: Vec<u8>,
    /// restart | backup | update | stop | start
    pub action: String,
}

impl Default for ScheduleEntry {
    fn default() -> Self {
        Self { enabled: true, time: "04:00".into(), days: vec![], action: "restart".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationSettings {
    pub auto_start: bool,
    pub auto_restart: bool,
    pub restart_delay_secs: u32,
    /// Crash-loop guard: stop auto-restarting after this many crashes within 15 minutes.
    pub crash_limit: u32,
    pub stop_timeout_secs: u32,
    pub schedules: Vec<ScheduleEntry>,
    pub backup_interval_mins: u32,
    pub backup_keep: u32,
    pub backup_on_stop: bool,
    pub backup_before_update: bool,
    pub update_on_start: bool,
    pub update_check_mins: u32,
    pub auto_update: bool,
    /// Wait for the server to be empty before an automatic update/restart (up to 60 minutes).
    pub wait_for_empty: bool,
    /// Restart when the server's memory exceeds this many MB (0 = off).
    pub memory_limit_mb: u32,
    /// Restart when the server has been up this many hours (0 = off).
    pub max_uptime_hours: u32,
    /// Stop the server after this many minutes with nobody online (0 = off).
    pub idle_stop_mins: u32,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        Self {
            auto_start: false,
            auto_restart: true,
            restart_delay_secs: 10,
            crash_limit: 3,
            stop_timeout_secs: 60,
            schedules: vec![],
            backup_interval_mins: 60,
            backup_keep: 24,
            backup_on_stop: true,
            backup_before_update: true,
            update_on_start: false,
            update_check_mins: 60,
            auto_update: false,
            wait_for_empty: false,
            memory_limit_mb: 0,
            max_uptime_hours: 0,
            idle_stop_mins: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerInstance {
    pub id: String,
    pub name: String,
    pub color: String,
    pub install_dir: PathBuf,
    pub created_at: i64,
    pub notes: String,
    /// Steam branch ("public" or a beta name like "public-test") and its password.
    pub branch: String,
    pub branch_password: String,
    pub config: ServerConfig,
    pub launch: LaunchSettings,
    pub automation: AutomationSettings,
}

impl Default for ServerInstance {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Valheim Server".into(),
            color: "fjord".into(),
            install_dir: PathBuf::new(),
            created_at: 0,
            notes: String::new(),
            branch: "public".into(),
            branch_password: String::new(),
            config: ServerConfig::default(),
            launch: LaunchSettings::default(),
            automation: AutomationSettings::default(),
        }
    }
}

/// A `./relative` or absolute folder from the config, resolved against the install folder.
fn resolve_dir(base: &Path, v: &str, fallback: &str) -> PathBuf {
    let v = v.trim();
    let v = if v.is_empty() { fallback } else { v };
    let p = PathBuf::from(v.replace('/', "\\"));
    if p.is_absolute() {
        p
    } else {
        base.join(v.trim_start_matches("./").trim_start_matches(".\\").replace('/', "\\"))
    }
}

impl ServerInstance {
    /// Every UDP port the server binds and players need: game + query (verified: 2456/2457).
    pub fn udp_ports(&self) -> Vec<u16> {
        vec![self.config.port, self.config.query_port()]
    }
    /// `-savedir`.
    pub fn data_dir(&self) -> PathBuf {
        resolve_dir(&self.install_dir, &self.launch.data_dir, "./save-data")
    }
    pub fn worlds_dir(&self) -> PathBuf {
        self.data_dir().join(WORLDS_DIR)
    }
    /// The folder of the world the server loads.
    pub fn world_dir(&self) -> PathBuf {
        self.worlds_dir().join(self.config.world.trim())
    }
    pub fn admin_path(&self) -> PathBuf {
        self.data_dir().join(ADMIN_FILE)
    }
    pub fn ban_path(&self) -> PathBuf {
        self.data_dir().join(BAN_FILE)
    }
    pub fn permit_path(&self) -> PathBuf {
        self.data_dir().join(PERMIT_FILE)
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.install_dir.join("logs")
    }
    pub fn log_path(&self) -> PathBuf {
        self.logs_dir().join(LOG_FILE)
    }
    pub fn server_exe(&self) -> PathBuf {
        resolve_server_exe(&self.install_dir)
    }
    pub fn is_installed(&self) -> bool {
        self.server_exe().exists()
    }
    /// `BepInEx` in the server folder (the Thunderstore BepInExPack_Valheim layout).
    pub fn bepinex_dir(&self) -> PathBuf {
        self.install_dir.join("BepInEx")
    }
    pub fn plugins_dir(&self) -> PathBuf {
        self.bepinex_dir().join("plugins")
    }
}

pub fn resolve_server_exe(install_dir: &Path) -> PathBuf {
    install_dir.join(SERVER_EXE)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ServerState {
    NotInstalled,
    Stopped,
    Installing,
    Updating,
    Starting,
    Running,
    Stopping,
    Restarting,
    Crashed,
}

impl ServerState {
    pub fn is_live(self) -> bool {
        matches!(self, ServerState::Starting | ServerState::Running | ServerState::Stopping)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub id: String,
    pub state: ServerState,
    pub pid: Option<u32>,
    pub started_at: Option<i64>,
    pub ready_at: Option<i64>,
    pub installed_build: Option<String>,
    pub latest_build: Option<String>,
    /// Game version from the log (`1.0.20`).
    pub game_version: Option<String>,
    /// In-game day of the loaded world (from its last save).
    pub day: Option<u32>,
    pub crash_count: u32,
    pub crash_loop: bool,
    pub pending_restart: bool,
    /// Players online (from the log; the count is confirmed by A2S).
    pub players: Vec<OnlinePlayer>,
    /// Player count reported by the server over A2S (None = no answer yet).
    pub query_players: Option<u32>,
    pub next_schedule: Option<NextSchedule>,
    pub last_backup: Option<i64>,
    pub last_exit_code: Option<i64>,
    pub busy: Option<String>,
    /// Steam game-server id from the log (`Server ID …`).
    pub steam_id: Option<String>,
    /// Public IPv4 the server registered with (crossplay session line).
    pub public_ip: Option<String>,
    /// Crossplay join code (`Session "…" registered with join code 123456`).
    pub join_code: Option<String>,
    /// The last world save the log reported.
    pub last_save: Option<i64>,
    /// An automatic restart/update/stop waiting for players to leave.
    pub waiting: Option<String>,
    /// BepInEx was loaded by this server process (mods active).
    pub modded: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OnlinePlayer {
    /// Character name (empty until the player's character spawns).
    pub name: String,
    pub since: i64,
    /// SteamID64 for Steam players.
    pub steam_id: Option<String>,
    /// `Platform_UserId` (what the permission lists hold), when known.
    pub platform_id: Option<String>,
    pub deaths: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSchedule {
    pub at: i64,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcSample {
    pub id: String,
    pub cpu: f32,
    pub mem: u64,
    pub virt: u64,
    pub read_bps: u64,
    pub write_bps: u64,
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemSample {
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub cores: usize,
    pub per_core: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEvent {
    pub ts: i64,
    pub system: SystemSample,
    pub servers: Vec<ProcSample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task_id: String,
    pub instance_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub message: String,
    pub progress: Option<f64>,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogBatch {
    pub id: String,
    /// server | manager | steamcmd | mods
    pub source: String,
    pub lines: Vec<String>,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEvent {
    pub id: String,
    /// info | success | warn | error
    pub level: String,
    pub message: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerEvent {
    pub id: String,
    pub name: String,
    /// join | leave | death
    pub kind: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub created_at: i64,
    pub label: String,
    pub kind: String,
    pub files: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFile {
    pub path: PathBuf,
    pub exists: bool,
    pub text: String,
    pub modified: Option<i64>,
    /// Text saved while the server ran – written once it exits.
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackManifest {
    pub format: u32,
    pub app_version: String,
    pub exported_at: i64,
    pub instance: ServerInstance,
    pub has_saves: bool,
    pub has_config: bool,
    pub includes_secrets: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagCheck {
    pub id: String,
    pub title: String,
    /// ok | warn | error | info
    pub level: String,
    pub detail: String,
    /// Fix the UI can trigger: firewall | install | validate | settings | admins | ports | update | log | mods | ""
    pub fix: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: PathBuf,
    pub portable: bool,
    pub app_id: u32,
    pub cores: usize,
    pub total_memory: u64,
    pub os: String,
    pub hostname: String,
    /// Secrets that couldn't be decrypted at load (portable folder moved to another PC/account).
    pub lost_secrets: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_resolve_relative_and_absolute() {
        let mut i = ServerInstance { install_dir: PathBuf::from(r"D:\srv\a"), ..Default::default() };
        assert_eq!(i.data_dir(), PathBuf::from(r"D:\srv\a\save-data"));
        assert_eq!(i.world_dir(), PathBuf::from(r"D:\srv\a\save-data\worlds_local\Dedicated"));
        assert_eq!(i.admin_path(), PathBuf::from(r"D:\srv\a\save-data\adminlist.txt"));
        assert_eq!(i.log_path(), PathBuf::from(r"D:\srv\a\logs\valheim_server.log"));
        i.launch.data_dir = r"E:\vh\one".into();
        assert_eq!(i.data_dir(), PathBuf::from(r"E:\vh\one"));
        i.launch.data_dir = "".into();
        assert_eq!(i.data_dir(), PathBuf::from(r"D:\srv\a\save-data"));
        assert_eq!(i.udp_ports(), vec![2456, 2457]);
        assert!(!i.config.has_modifiers());
        i.config.keys.push("nomap".into());
        assert!(i.config.has_modifiers());
    }
}
