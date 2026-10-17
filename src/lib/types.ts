// Mirrors src-tauri/src/models.rs (serde camelCase).

export interface AppSettings {
  steamcmdDir: string | null;
  defaultInstallRoot: string | null;
  backupDir: string | null;
  lastDataDir?: string | null;
  telemetryIntervalMs: number;
  confirmDestructive: boolean;
  reduceMotion: boolean;
  checkUpdatesOnLaunch: boolean;
  accent: string;
  closeToTray: boolean;
  desktopNotifications: boolean;
  streamerMode: boolean;
  webhookUrl: string;
  webhookLevels: string[];
  metricsRetentionDays: number;
}

export type SettingValue = number | string | boolean;

/** Everything on the server's command line (models.rs ServerConfig). */
export interface ServerConfig {
  /** -name: shown in the server browser. */
  name: string;
  /** -port: the server uses this UDP port and the next one (query). */
  port: number;
  /** -world: the world under worlds_local the server loads. */
  world: string;
  /** -password: at least 5 characters, not part of the name; empty = open. */
  password: string;
  /** -public 1|0 */
  public: boolean;
  /** -crossplay: PlayFab backend, join codes, no port forwarding. */
  crossplay: boolean;
  instanceId: string;
  saveInterval: number;
  backups: number;
  backupShort: number;
  backupLong: number;
  /** -preset; '' = keep the world's stored modifiers. */
  preset: string;
  /** -modifier key value; '' / missing = the preset's value. */
  modifiers: Record<string, string>;
  /** -setkey checkboxes. */
  keys: string[];
}

export interface LaunchSettings {
  /** -savedir; relative = inside the install folder. */
  dataDir: string;
  upnp: boolean;
  showConsole: boolean;
  priority: 'idle' | 'belowNormal' | 'normal' | 'aboveNormal' | 'high' | string;
  affinity: number[];
  extraArgs: string;
  /** Load BepInEx mods (when installed). */
  modsEnabled: boolean;
}

export type ScheduleAction = 'restart' | 'backup' | 'update' | 'stop' | 'start';

export interface ScheduleEntry {
  enabled: boolean;
  time: string;
  days: number[];
  action: ScheduleAction | string;
}

export interface AutomationSettings {
  autoStart: boolean;
  autoRestart: boolean;
  restartDelaySecs: number;
  crashLimit: number;
  stopTimeoutSecs: number;
  schedules: ScheduleEntry[];
  backupIntervalMins: number;
  backupKeep: number;
  backupOnStop: boolean;
  backupBeforeUpdate: boolean;
  updateOnStart: boolean;
  updateCheckMins: number;
  autoUpdate: boolean;
  waitForEmpty: boolean;
  memoryLimitMb: number;
  maxUptimeHours: number;
  idleStopMins: number;
}

export interface ServerInstance {
  id: string;
  name: string;
  color: string;
  installDir: string;
  createdAt: number;
  notes: string;
  branch: string;
  branchPassword: string;
  config: ServerConfig;
  launch: LaunchSettings;
  automation: AutomationSettings;
}

export type ServerState = 'notInstalled' | 'stopped' | 'installing' | 'updating' | 'starting' | 'running' | 'stopping' | 'restarting' | 'crashed';

export interface OnlinePlayer {
  /** Character name – empty while the player is still connecting. */
  name: string;
  since: number;
  steamId: string | null;
  /** Steam_7656… / Xbox_… as the permission lists use it. */
  platformId: string | null;
  deaths: number;
}

export interface ServerStatus {
  id: string;
  state: ServerState;
  pid: number | null;
  startedAt: number | null;
  readyAt: number | null;
  installedBuild: string | null;
  latestBuild: string | null;
  gameVersion: string | null;
  /** In-game day of the loaded world (at its last save). */
  day: number | null;
  crashCount: number;
  crashLoop: boolean;
  pendingRestart: boolean;
  players: OnlinePlayer[];
  queryPlayers: number | null;
  nextSchedule: { at: number; action: string } | null;
  lastBackup: number | null;
  lastExitCode: number | null;
  busy: string | null;
  steamId: string | null;
  publicIp: string | null;
  /** Crossplay join code. */
  joinCode: string | null;
  lastSave: number | null;
  waiting: string | null;
  /** BepInEx was loaded by the running server. */
  modded: boolean;
}

export interface ProcSample {
  id: string;
  cpu: number;
  mem: number;
  virt: number;
  readBps: number;
  writeBps: number;
  uptime: number;
}

export interface SystemSample {
  cpu: number;
  memUsed: number;
  memTotal: number;
  rxBps: number;
  txBps: number;
  cores: number;
  perCore: number[];
}

export interface TelemetryEvent {
  ts: number;
  system: SystemSample;
  servers: ProcSample[];
}

export interface TaskEvent {
  taskId: string;
  instanceId: string | null;
  kind: string;
  title: string;
  message: string;
  progress: number | null;
  done: boolean;
  error: string | null;
}

export interface LogBatch {
  id: string;
  source: 'server' | 'manager' | 'steamcmd' | string;
  lines: string[];
  ts: number;
}

export interface ServerEvent {
  id: string;
  level: 'info' | 'success' | 'warn' | 'error';
  message: string;
  ts: number;
}

export interface PlayerEvent {
  id: string;
  name: string;
  kind: 'join' | 'leave' | 'death';
  ts: number;
}

export interface BackupInfo {
  path: string;
  name: string;
  size: number;
  createdAt: number;
  label: string;
  kind: string;
  files: number;
}

export interface SaveCheck {
  /** "Current save" or a backup's name. */
  file: string;
  ok: boolean;
  bytes: number;
  error: string | null;
}

export interface VerifyResult {
  ok: boolean;
  files: number;
  worlds: string[];
  hasSettings: boolean;
  bytes: number;
  error: string | null;
}

export interface RulePreset {
  name: string;
  createdAt: number;
  /** A saved set of world modifiers. */
  settings: { preset?: string; modifiers?: Record<string, string>; keys?: string[] };
}

export interface WorldMeta {
  name: string;
  seedName: string;
  seed: number;
  version: number;
  /** Starting global keys: `preset combat_hard:…`, `nomap`, `playerdamage 85` … */
  keys: string[];
}

export interface WorldBackup {
  name: string;
  kind: 'auto' | 'restore' | 'cloud' | 'manual' | string;
  createdAt: number;
  size: number;
}

export interface World {
  name: string;
  format: 'folder' | 'legacy';
  active: boolean;
  saveNumber: number | null;
  meta: WorldMeta | null;
  day: number | null;
  savedAt: number | null;
  size: number;
  path: string;
  backups: WorldBackup[];
  hasOld: boolean;
}

export interface LocalWorld {
  world: World;
  source: string;
}

export interface ConfigFile {
  path: string;
  exists: boolean;
  text: string;
  modified: number | null;
  pending: boolean;
}

export type ConfigFileKind = 'admins' | 'bans' | 'permitted';

export interface ConfigIssue {
  level: 'error' | 'warn' | 'info';
  message: string;
}

export interface AccessEntry {
  /** Steam_7656… / Xbox_… as written in the list. */
  id: string;
  steamId: string | null;
  name: string | null;
  online: boolean;
}

export type AccessList = 'admins' | 'bans' | 'permitted';

export interface AccessView {
  admins: AccessEntry[];
  bans: AccessEntry[];
  permitted: AccessEntry[];
}

export interface PortSet {
  port: number;
  queryPort: number;
}

export interface ModInfo {
  id: string;
  name: string;
  author: string | null;
  version: string | null;
  description: string | null;
  website: string | null;
  package: string | null;
  dependencies: string[];
  enabled: boolean;
  dlls: number;
  size: number;
  modified: number;
}

export interface ModsStatus {
  bepinex: boolean;
  bepinexVersion: string | null;
  proxyActive: boolean;
  mods: ModInfo[];
  configs: { name: string; size: number; modified: number }[];
  hasLog: boolean;
}

export interface ModUpdate {
  id: string;
  installed: string | null;
  latest: string;
}

export interface PackManifest {
  format: number;
  appVersion: string;
  exportedAt: number;
  instance: ServerInstance;
  hasSaves: boolean;
  hasConfig: boolean;
  includesSecrets: boolean;
}

export interface DiagCheck {
  id: string;
  title: string;
  level: 'ok' | 'warn' | 'error' | 'info';
  detail: string;
  fix: '' | 'firewall' | 'install' | 'validate' | 'settings' | 'admins' | 'ports' | 'update' | 'log' | 'mods' | string;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  portable: boolean;
  appId: number;
  cores: number;
  totalMemory: number;
  os: string;
  hostname: string;
  lostSecrets: string[];
}

export interface FolderProbe {
  exists: boolean;
  writable: boolean;
  empty: boolean;
  hasServer: boolean;
  freeBytes: number | null;
}

export interface FirewallState {
  ruleExists: boolean;
  portsOk: boolean;
  rulePorts: string;
  programRule: boolean;
  blockedBy: string[];
}

export interface NetInfo {
  publicIp: string | null;
  local: { interface: string; ip: string }[];
  gateway: string | null;
}

export interface UpnpMapping {
  externalPort: number;
  internalPort: number;
  internalClient: string;
  description: string;
  enabled: boolean;
}

export interface UpnpInfo {
  found: boolean;
  gateway: string | null;
  routerName: string | null;
  manufacturer: string | null;
  model: string | null;
  externalIp: string | null;
  wanKind: 'public' | 'private' | 'cgnat' | 'unknown';
  mappings: UpnpMapping[];
  error: string | null;
}

export interface SteamListing {
  addr: string;
  gameport: number;
  appid: number;
  steamid: string;
}

export interface A2sInfo {
  name: string;
  players: number;
  maxPlayers: number;
  version: string;
  password: boolean;
  keywords: string;
  pingMs: number;
}

export interface QueryResult {
  info: A2sInfo | null;
  players: { name: string; score: number; durationSecs: number }[];
}

export interface LogFile {
  path: string;
  name: string;
  size: number;
  modified: number;
}

export interface DiskUsage {
  install: number;
  saves: number;
  backups: number;
  logs: number;
}

export interface MetricRow {
  ts: number;
  cpu: number;
  mem: number;
  readBps: number;
  writeBps: number;
  players: number;
}

export interface EventRow {
  id: number;
  serverId: string | null;
  ts: number;
  level: 'info' | 'success' | 'warn' | 'error' | string;
  kind: string;
  message: string;
}

export interface SessionRow {
  id: number;
  name: string;
  joinedAt: number;
  leftAt: number | null;
  steamId: string | null;
}

export interface PlayerSummary {
  name: string;
  sessions: number;
  totalMs: number;
  firstSeen: number;
  lastSeen: number;
  steamId: string | null;
}

export interface BuildRow {
  buildId: string;
  branch: string;
  ts: number;
}
