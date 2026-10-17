// Fake backend for `npm run dev` in a plain browser. Never loaded inside Tauri (see main.tsx).
// Log lines and file shapes are copied from a real valheim_server.exe 1.0.20 run (build 25909431).
import { installMock, mockEmit } from './ipc';
import { defaultConfig } from './defaults';
import type { AccessEntry, AccessView, AppSettings, BackupInfo, EventRow, MetricRow, ModsStatus, RulePreset, ServerConfig, ServerInstance, ServerStatus, World } from './types';

const now = Date.now();
const STAMP = (t = Date.now()) => {
  const d = new Date(t);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getMonth() + 1)}/${p(d.getDate())}/${d.getFullYear()} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
};

function makeInstance(id: string, name: string, port: number, color: string, extra: Partial<ServerInstance> = {}): ServerInstance {
  return {
    id,
    name,
    color,
    installDir: `D:\\ValheimServers\\${name.replace(/\W+/g, '-')}`,
    createdAt: now - 86400000 * 3,
    notes: '',
    branch: 'public',
    branchPassword: '',
    config: defaultConfig({ name, port, world: 'Midgard', password: 'skal-mead-7' }),
    launch: { dataDir: './save-data', upnp: false, showConsole: false, priority: 'normal', affinity: [], extraArgs: '', modsEnabled: true },
    automation: {
      autoStart: false,
      autoRestart: true,
      restartDelaySecs: 10,
      crashLimit: 3,
      stopTimeoutSecs: 60,
      schedules: [{ enabled: true, time: '05:00', days: [], action: 'restart' }],
      backupIntervalMins: 60,
      backupKeep: 24,
      backupOnStop: true,
      backupBeforeUpdate: true,
      updateOnStart: false,
      updateCheckMins: 60,
      autoUpdate: false,
      waitForEmpty: false,
      memoryLimitMb: 0,
      maxUptimeHours: 0,
      idleStopMins: 0,
    },
    ...extra,
  };
}

let instances: ServerInstance[] = [makeInstance('a1b2c3d4-0001', 'Hearth & Hall', 2456, 'ember'), makeInstance('a1b2c3d4-0002', 'Hardcore Fjords', 2459, 'fjord')];
instances[1].config = { ...instances[1].config, world: 'Niflheim', preset: 'hardcore', modifiers: { raids: 'muchmore' }, keys: ['nomap'], crossplay: true };
let rulePresets: RulePreset[] = [{ name: 'Cozy building season', createdAt: Date.now() - 86400_000 * 3, settings: { preset: 'casual', modifiers: { resources: 'more' }, keys: ['nobuildcost'] } }];

const statuses: Record<string, ServerStatus> = {};
function status(id: string, patch: Partial<ServerStatus> = {}): ServerStatus {
  const s: ServerStatus = statuses[id] ?? {
    id,
    state: 'stopped',
    pid: null,
    startedAt: null,
    readyAt: null,
    installedBuild: '25909431',
    latestBuild: '25909431',
    gameVersion: null,
    day: 37,
    crashCount: 0,
    crashLoop: false,
    pendingRestart: false,
    players: [],
    queryPlayers: null,
    nextSchedule: { at: now + 3600_000 * 5, action: 'restart' },
    lastBackup: now - 1800_000,
    lastExitCode: null,
    busy: null,
    steamId: null,
    publicIp: null,
    joinCode: null,
    lastSave: null,
    waiting: null,
    modded: false,
  };
  statuses[id] = { ...s, ...patch };
  return statuses[id];
}
status('a1b2c3d4-0001', {
  state: 'running',
  pid: 18580,
  startedAt: now - 3600_000 * 2.3,
  readyAt: now - 3600_000 * 2.2,
  gameVersion: '1.0.20',
  day: 142,
  steamId: '90071992547409920',
  lastSave: now - 600_000,
  queryPlayers: 3,
  modded: true,
  players: [
    { name: 'Bjørn Ironside', since: now - 3600_000 * 1.2, steamId: '76561198000000001', platformId: 'Steam_76561198000000001', deaths: 2 },
    { name: 'Freya', since: now - 1200_000, steamId: '76561198000000002', platformId: 'Steam_76561198000000002', deaths: 0 },
    { name: '', since: now - 20_000, steamId: '76561198000000009', platformId: 'Steam_76561198000000009', deaths: 0 },
  ],
});
status('a1b2c3d4-0002', { day: 3 });

let settings: AppSettings = {
  steamcmdDir: null,
  defaultInstallRoot: null,
  backupDir: null,
  telemetryIntervalMs: 1000,
  confirmDestructive: true,
  reduceMotion: false,
  checkUpdatesOnLaunch: true,
  accent: 'fjord',
  closeToTray: true,
  desktopNotifications: true,
  streamerMode: false,
  webhookUrl: '',
  webhookLevels: ['success', 'error'],
  metricsRetentionDays: 30,
};

const entry = (id: string, name: string | null, online = false): AccessEntry => ({ id, steamId: id.startsWith('Steam_') ? id.slice(6) : null, name, online });
const access: Record<string, AccessView> = {
  'a1b2c3d4-0001': {
    admins: [entry('Steam_76561198000000001', 'Bjørn Ironside', true)],
    bans: [entry('Steam_76561198000000666', 'Griefer')],
    permitted: [],
  },
};

const mods: ModsStatus = {
  bepinex: true,
  bepinexVersion: '5.4.2351',
  proxyActive: true,
  hasLog: true,
  mods: [
    { id: 'ValheimModding-Jotunn', name: 'Jotunn', author: 'ValheimModding', version: '2.30.2', description: 'Jötunn, the Valheim Library.', website: '', package: 'ValheimModding-Jotunn', dependencies: ['denikson-BepInExPack_Valheim-5.4.2333'], enabled: true, dlls: 1, size: 2_310_000, modified: now - 86400_000 * 4 },
    { id: 'Azumatt-ServerDevcommands', name: 'Server devcommands', author: 'Azumatt', version: '1.88.0', description: 'Admin commands on a dedicated server.', website: '', package: 'Azumatt-ServerDevcommands', dependencies: [], enabled: true, dlls: 1, size: 640_000, modified: now - 86400_000 * 2 },
    { id: 'MyTweaks', name: 'MyTweaks', author: null, version: null, description: null, website: null, package: null, dependencies: [], enabled: false, dlls: 1, size: 18_000, modified: now - 86400_000 * 9 },
  ],
  configs: [
    { name: 'BepInEx.cfg', size: 6100, modified: now - 86400_000 * 4 },
    { name: 'com.jotunn.jotunn.cfg', size: 1400, modified: now - 86400_000 * 4 },
    { name: 'server_devcommands.cfg', size: 5300, modified: now - 86400_000 * 2 },
  ],
};

const backups: BackupInfo[] = Array.from({ length: 6 }, (_, i) => ({
  path: `D:\\ValheimServers\\Hearth-Hall\\Backups\\2026-10-0${9 - i}_auto.zip`,
  name: `2026-10-0${9 - i}_04-00-00_${i === 2 ? 'manual' : 'auto'}.zip`,
  size: 21_100_000 + i * 320_000,
  createdAt: now - i * 3600_000 * 7,
  label: i === 2 ? 'Before Moder' : 'Interval',
  kind: i === 2 ? 'manual' : 'auto',
  files: 41,
}));

const META_KEYS = ['preset combat_hard:deathpenalty_default:resources_more:raids_none:portals_casual', 'nomap'];
const world = (path: string, name: string, active: boolean, age: number, day: number, backupsN = 3): World => ({
  name,
  format: 'folder',
  active,
  saveNumber: 1214 - backupsN,
  meta: { name, seedName: active ? 'Jk5aSQlv4V' : 'HuginnMun', seed: 685881191, version: 42, keys: active ? META_KEYS : [] },
  day,
  savedAt: now - age,
  size: 48_000_000 + day * 120_000,
  path,
  backups: Array.from({ length: backupsN }, (_, k) => ({ name: `${name}_backup_auto-202610${16 - k}-0${4 + k}1500`, kind: 'auto', createdAt: now - age - k * 43_200_000 - 7_200_000, size: 46_000_000 - k * 400_000 })),
  hasOld: false,
});

const LOG_SAMPLES = [
  `${STAMP()}: World save (1/5) Cloud & Backup checks done [0ms] => Save number 1215`,
  `${STAMP()}: World save (5/5) done. Total time [412ms]`,
  `${STAMP()}: Got connection SteamID 76561198000000002`,
  `${STAMP()}: Got character ZDOID from Freya : -1934829:1`,
  `${STAMP()}: Got character ZDOID from Bjørn Ironside : 0:0`,
  `${STAMP()}: Failed to place all SwampRuin2, placed 7 out of 30 with 0 tries in 00:00:00.0120027`,
];

function emitTelemetry() {
  const t = Date.now() / 1000;
  const running = instances.filter((i) => statuses[i.id]?.state === 'running' || statuses[i.id]?.state === 'starting');
  mockEmit('telemetry', {
    ts: Date.now(),
    system: {
      cpu: 22 + Math.sin(t / 7) * 8 + Math.random() * 6,
      memUsed: 24.2e9 + Math.sin(t / 30) * 4e8,
      memTotal: 68.4e9,
      rxBps: 180_000 + Math.random() * 90_000,
      txBps: 260_000 + Math.random() * 120_000,
      cores: 24,
      perCore: Array.from({ length: 24 }, (_, i) => Math.max(1, 25 + Math.sin(t / 3 + i) * 20 + Math.random() * 12)),
    },
    servers: running.map((i, k) => ({
      id: i.id,
      cpu: 6 + k * 3 + Math.sin(t / 5) * 2.5 + Math.random() * 2,
      mem: 2.1e9 + k * 6e8 + Math.sin(t / 40) * 1.2e8,
      virt: 4.6e9,
      readBps: Math.random() < 0.2 ? Math.random() * 2e6 : Math.random() * 40_000,
      writeBps: Math.random() < 0.1 ? 4e6 + Math.random() * 3e6 : Math.random() * 120_000,
      uptime: Math.round((Date.now() - (statuses[i.id].startedAt ?? Date.now())) / 1000),
    })),
  });
}

function emitStatus(id: string, patch: Partial<ServerStatus>) {
  mockEmit('server-status', status(id, patch));
}

function log(id: string, source: string, line: string) {
  mockEmit('log-lines', { id, source, lines: [line], ts: Date.now() });
}

const events: EventRow[] = [
  { id: 6, serverId: 'a1b2c3d4-0001', ts: now - 1200_000, level: 'info', kind: 'player', message: 'Freya joined Hearth & Hall' },
  { id: 5, serverId: 'a1b2c3d4-0001', ts: now - 3600_000 * 1.2, level: 'info', kind: 'player', message: 'Bjørn Ironside joined Hearth & Hall' },
  { id: 4, serverId: 'a1b2c3d4-0001', ts: now - 3600_000 * 2.2, level: 'success', kind: 'lifecycle', message: 'Hearth & Hall is online' },
  { id: 3, serverId: 'a1b2c3d4-0001', ts: now - 3600_000 * 2.3, level: 'success', kind: 'update', message: 'Updated to build 25909431' },
  { id: 2, serverId: 'a1b2c3d4-0001', ts: now - 3600_000 * 26, level: 'success', kind: 'mods', message: 'Installed ValheimModding-Jotunn-2.30.2' },
  { id: 1, serverId: 'a1b2c3d4-0001', ts: now - 3600_000 * 30, level: 'info', kind: 'config', message: 'Admin added: Steam_76561198000000001' },
];

function metrics(since: number): MetricRow[] {
  const out: MetricRow[] = [];
  for (let ts = Math.max(since, now - 7 * 86400_000); ts < now; ts += 60_000 * 5) {
    const h = new Date(ts).getHours();
    const evening = h >= 18 && h <= 23 ? 1 : h >= 12 ? 0.5 : 0.1;
    const players = Math.max(0, Math.min(10, Math.round(evening * 6 + Math.sin(ts / 3.6e6) * 2)));
    out.push({ ts, cpu: 4 + players * 1.4 + Math.random(), mem: 2.0e9 + players * 1.2e8, readBps: Math.random() * 6e4, writeBps: Math.random() * 1.6e5, players });
  }
  return out;
}

const listText = (file: string, ids: AccessEntry[]) =>
  [`// List ${file === 'admins' ? 'admin' : file === 'bans' ? 'banned' : 'permitted'} players ID ${file === 'permitted' ? '' : ' '}ONE per line`, ...ids.map((x) => x.id)].join('\n') + '\n';

const CFG = `## Settings file was created by plugin Server devcommands v1.88.0
## Plugin GUID: server_devcommands

[1. General]

## Disables debug mode for admins.
# Setting type: Boolean
# Default value: false
Disable debug mode = false

## Automatic execution of commands on server start.
# Setting type: String
# Default value:
Auto exec = save
`;

export function installDevMock() {
  const find = (id: unknown) => instances.find((i) => i.id === id)!;
  const acc = (id: unknown) => (access[id as string] ??= { admins: [], bans: [], permitted: [] });
  installMock({
    get_app_info: () => ({ version: '1.0.0', dataDir: 'C:\\Users\\me\\AppData\\Roaming\\com.valheimservermanager.app', portable: false, appId: 896660, cores: 24, totalMemory: 68.4e9, os: 'Windows 11 Pro', hostname: 'LONGHOUSE-PC', lostSecrets: [] }),
    get_settings: () => settings,
    save_settings: ({ settings: s }) => void (settings = s as AppSettings),
    cancel_task: () => undefined,
    test_webhook: () => undefined,
    app_quit: () => undefined,
    generate_password: () => 'Rk7mPzVb4q',
    window_hide: () => undefined,
    probe_folder: () => ({ exists: true, writable: true, empty: true, hasServer: false, freeBytes: 812e9 }),
    list_instances: () => instances,
    get_statuses: () => instances.map((i) => status(i.id)),
    suggest_ports: () => ({ port: 2456 + instances.length * 3, queryPort: 2457 + instances.length * 3 }),
    create_instance: ({ input }) => {
      const inp = input as { name: string; color: string; config: ServerConfig; installNow: boolean };
      const inst = makeInstance(`mock-${Date.now()}`, inp.name, inp.config.port, inp.color);
      inst.config = inp.config;
      instances = [...instances, inst];
      status(inst.id, { state: inp.installNow ? 'installing' : 'notInstalled', installedBuild: null, day: null });
      if (inp.installNow) {
        const task = `t-${Date.now()}`;
        let p = 0;
        const iv = setInterval(() => {
          p += 0.1;
          mockEmit('task', { taskId: task, instanceId: inst.id, kind: 'steamcmd', title: `Installing ${inst.name}`, message: `Downloading – ${(p * 100).toFixed(1)}% (${(p * 1.1).toFixed(2)} / 1.10 GB)`, progress: Math.min(1, p), done: p >= 1, error: null });
          if (p >= 1) {
            clearInterval(iv);
            emitStatus(inst.id, { state: 'stopped', installedBuild: '25909431' });
          }
        }, 400);
      }
      return inst;
    },
    update_instance: ({ instance }) => {
      const i = instance as ServerInstance;
      instances = instances.map((x) => (x.id === i.id ? i : x));
      if (statuses[i.id]?.state === 'running') emitStatus(i.id, { pendingRestart: true });
      return i;
    },
    validate_config: ({ config }) => {
      const c = config as ServerConfig;
      const out: { level: string; message: string }[] = [];
      if (c.password && c.password.length < 5) out.push({ level: 'error', message: 'The password must be at least 5 characters (the server refuses shorter ones).' });
      return out;
    },
    reorder_instances: () => undefined,
    delete_instance: ({ id }) => void (instances = instances.filter((i) => i.id !== id)),
    clone_instance: ({ id, name }) => {
      const src = find(id);
      const c = { ...structuredClone(src), id: `mock-${Date.now()}`, name: name as string, config: { ...structuredClone(src.config), port: 2456 + instances.length * 3 } };
      instances = [...instances, c];
      status(c.id, { state: 'notInstalled', installedBuild: null });
      return c;
    },
    find_existing_installs: () => ['G:\\SteamLibrary\\steamapps\\common\\Valheim dedicated server'],
    import_existing: ({ path }) => {
      const c = makeInstance(`mock-${Date.now()}`, 'My server', 2465, 'moss', { installDir: path as string });
      instances = [...instances, c];
      status(c.id);
      return c;
    },
    install_server: ({ id }) => {
      emitStatus(id as string, { state: 'updating' });
      setTimeout(() => emitStatus(id as string, { state: 'stopped' }), 2500);
    },
    check_update: () => false,
    start_server: ({ id }) => {
      const sid = id as string;
      const i = find(sid);
      emitStatus(sid, { state: 'starting', pid: 4242, startedAt: Date.now(), players: [] });
      log(sid, 'manager', `Launching valheim_server.exe (PID 4242) -nographics -batchmode -name "${i.config.name}" -port ${i.config.port} -world "${i.config.world}" -password "********" -public 1 …`);
      setTimeout(() => {
        log(sid, 'server', `${STAMP()}: Valheim version: 1.0.20 (network version 41)`);
        log(sid, 'server', `${STAMP()}: ZNet.LoadWorld: ${i.config.world} (${i.config.world}), save number 1214`);
        log(sid, 'server', `${STAMP()}: Opened Steam server`);
        log(sid, 'server', `${STAMP()}: Game server connected`);
        emitStatus(sid, { state: 'running', readyAt: Date.now(), queryPlayers: 0, gameVersion: '1.0.20', joinCode: i.config.crossplay ? '748486' : null, publicIp: i.config.crossplay ? '203.0.113.42' : null });
        mockEmit('server-event', { id: sid, level: 'success', message: `${i.name} is online`, ts: Date.now() });
      }, 2500);
    },
    stop_server: ({ id }) => {
      const sid = id as string;
      emitStatus(sid, { state: 'stopping' });
      log(sid, 'manager', 'Asking the server to save and shut down…');
      setTimeout(() => {
        log(sid, 'server', `${STAMP()}: Game - OnApplicationQuit`);
        log(sid, 'server', `${STAMP()}: World save (5/5) done. Total time [35ms]`);
        log(sid, 'manager', 'Server saved and stopped.');
        emitStatus(sid, { state: 'stopped', pid: null, startedAt: null, players: [], queryPlayers: null, pendingRestart: false, joinCode: null });
      }, 1800);
    },
    restart_server: ({ id }) => {
      const sid = id as string;
      emitStatus(sid, { state: 'restarting' });
      setTimeout(() => emitStatus(sid, { state: 'running', startedAt: Date.now(), pendingRestart: false }), 3000);
    },
    cancel_deferred: ({ id }) => emitStatus(id as string, { waiting: null }),
    get_launch_line: ({ id }) => {
      const i = find(id);
      return `"${i.installDir}\\valheim_server.exe" -nographics -batchmode -name "${i.config.name}" -port ${i.config.port} -world "${i.config.world}" -password "********" -public 1 -savedir "${i.installDir}\\save-data" -logFile "${i.installDir}\\logs\\valheim_server.log" -saveinterval 1800 -backups 4 -backupshort 7200 -backuplong 43200`;
    },
    set_console_visible: () => 1,
    clear_crash_state: () => undefined,
    query_server: ({ id }) => ({
      info: statuses[id as string]?.state === 'running' ? { name: find(id).config.name, players: statuses[id as string].queryPlayers ?? 0, maxPlayers: 10, version: '1.0.0.0', password: !!find(id).config.password, keywords: 'g=1.0.20,n=41,m=', pingMs: 1 } : null,
      players: (statuses[id as string]?.players ?? []).filter((p) => p.name).map((p) => ({ name: p.name, score: 0, durationSecs: (Date.now() - p.since) / 1000 })),
    }),
    read_config: ({ id, file }) => {
      const i = find(id);
      const a = acc(id);
      const list = file === 'admins' ? a.admins : file === 'bans' ? a.bans : a.permitted;
      const name = { admins: 'adminlist.txt', bans: 'bannedlist.txt', permitted: 'permittedlist.txt' }[file as string];
      return { path: `${i.installDir}\\save-data\\${name}`, exists: true, text: listText(file as string, list), modified: now - 60000, pending: false };
    },
    write_config: ({ id }) => ({ restartNeeded: false, instance: find(id) }),
    list_config_history: () => [['D:\\x\\_config-history\\adminlist.20261009-101500.txt', now - 3600_000]],
    read_text_file: ({ id }) => listText('admins', acc(id).admins),
    get_access: ({ id }) => acc(id),
    set_access: ({ id, list, playerId, add }) => {
      const a = acc(id);
      const arr = list === 'admins' ? a.admins : list === 'bans' ? a.bans : a.permitted;
      const pid = /^7656\d{13}$/.test(playerId as string) ? `Steam_${playerId}` : (playerId as string);
      const has = arr.some((x) => x.id === pid);
      if (add && !has) arr.push(entry(pid, null));
      if (!add && has) arr.splice(arr.findIndex((x) => x.id === pid), 1);
      return add !== has;
    },
    read_log_tail: () => [
      "Mono path[0] = 'D:/ValheimServers/Hearth-Hall/valheim_server_Data/Managed'",
      'Initialize engine version: 6000.0.75f1 (26349cd2a5c8)',
      `${STAMP(now - 3600_000 * 2.3)}: Setting -savedir to: D:\\ValheimServers\\Hearth-Hall\\save-data`,
      `${STAMP(now - 3600_000 * 2.3)}: Using environment steamid 892970`,
      `${STAMP(now - 3600_000 * 2.3)}: Server ID 90071992547409920`,
      `${STAMP(now - 3600_000 * 2.3)}: Valheim version: 1.0.20 (network version 41)`,
      `${STAMP(now - 3600_000 * 2.3)}: isModded: True`,
      '[Message:   BepInEx] BepInEx 5.4.23.2 - valheim_server',
      `${STAMP(now - 3600_000 * 2.3)}: ZNet.LoadWorld: Midgard (Midgard), save number 1214`,
      `${STAMP(now - 3600_000 * 2.2)}: Opened Steam server`,
      `${STAMP(now - 3600_000 * 2.2)}: Game server connected`,
      `${STAMP(now - 3600_000 * 1.2)}: Got connection SteamID 76561198000000001`,
      `${STAMP(now - 3600_000 * 1.2)}: Got character ZDOID from Bjørn Ironside : -1934829:1`,
      ...LOG_SAMPLES,
    ],
    list_log_files: () => [
      { path: 'D:\\x\\logs\\backup\\valheim_server_2026-10-14T07-14-49.log', name: 'valheim_server_2026-10-14T07-14-49.log', size: 63_168, modified: now - 86400_000 },
      { path: 'D:\\x\\BepInEx\\LogOutput.log', name: 'BepInEx LogOutput.log', size: 22_400, modified: now - 3600_000 * 2.3 },
    ],
    list_worlds: ({ id }) => {
      const i = find(id);
      const base = `${i.installDir}\\save-data\\worlds_local`;
      return i.id === 'a1b2c3d4-0001' ? [world(`${base}\\Midgard`, 'Midgard', true, 600_000, 142), world(`${base}\\Season-1`, 'Season-1', false, 86400_000 * 20, 311, 4)] : [];
    },
    list_local_worlds: () => [{ world: world('C:\\Users\\me\\AppData\\LocalLow\\IronGate\\Valheim\\worlds_local\\MySoloWorld', 'MySoloWorld', false, 86400_000 * 2, 58, 1), source: 'Local worlds' }],
    scan_world_folder: ({ path }) => [world(path as string, 'OldServerWorld', false, 86400_000 * 5, 220, 0)],
    import_world: ({ name }) => ({ name, files: 41 }),
    restore_world_backup: ({ backup }) => backup,
    export_world: () => 41,
    duplicate_world: ({ newName }) => newName,
    rename_world: ({ newName }) => newName,
    check_world: () => [
      { file: 'Current save', ok: true, bytes: 61_200_512, error: null },
      { file: 'Midgard_backup_auto-20261016-041500', ok: true, bytes: 60_800_000, error: null },
      { file: 'Midgard_backup_auto-20261015-051500', ok: false, bytes: 0, error: 'damaged (corrupt deflate stream)' },
      { file: 'Midgard_backup_auto-20261014-061500', ok: true, bytes: 59_900_000, error: null },
    ],
    list_rule_presets: () => rulePresets,
    save_rule_preset: ({ name, settings }) => {
      const p = { name: name as string, createdAt: Date.now(), settings: settings as RulePreset['settings'] };
      rulePresets = [...rulePresets.filter((x) => x.name.toLowerCase() !== p.name.toLowerCase()), p];
      return p;
    },
    delete_rule_preset: ({ name }) => {
      rulePresets = rulePresets.filter((x) => x.name !== name);
    },
    delete_world: () => undefined,
    reset_world: () => null,
    activate_world: ({ id, name }) => {
      const i = find(id);
      i.config = { ...i.config, world: name as string };
      return i;
    },
    mods_status: ({ id }) => (id === 'a1b2c3d4-0001' ? mods : { bepinex: false, bepinexVersion: null, proxyActive: false, mods: [], configs: [], hasLog: false }),
    install_bepinex: () => undefined,
    uninstall_bepinex: () => 'D:\\x\\_mods-removed\\20261016-BepInEx',
    install_mod: ({ id, spec }) => {
      const task = `m-${Date.now()}`;
      setTimeout(() => mockEmit('task', { taskId: task, instanceId: id, kind: 'mods', title: `Installing ${spec}`, message: 'Installed', progress: 1, done: true, error: null }), 1200);
    },
    install_mod_file: () => 'MyMod',
    set_mod_enabled: ({ modId, enabled }) => {
      const m = mods.mods.find((x) => x.id === modId);
      if (m) m.enabled = enabled as boolean;
    },
    remove_mod: ({ modId }) => {
      mods.mods = mods.mods.filter((x) => x.id !== modId);
    },
    check_mod_updates: () => [{ id: 'Azumatt-ServerDevcommands', installed: '1.88.0', latest: '1.89.1' }],
    read_mod_config: ({ name }) => ({ path: `D:\\ValheimServers\\Hearth-Hall\\BepInEx\\config\\${name}`, exists: true, text: CFG, modified: now - 86400_000, pending: false }),
    write_mod_config: () => false,
    mod_config_history: () => [],
    mods_log: () => [
      '[Message:   BepInEx] BepInEx 5.4.23.2 - valheim_server (10/16/2026 8:10:20 AM)',
      '[Info   :   BepInEx] Running under Unity 6000.0.75f1',
      '[Info   :   BepInEx] 2 plugins to load',
      '[Info   :   BepInEx] Loading [Jotunn 2.30.2]',
      '[Info   :   BepInEx] Loading [Server devcommands 1.88.0]',
      '[Message:   BepInEx] Chainloader startup complete',
    ],
    metrics_history: ({ since }) => metrics(since as number),
    event_history: ({ id }) => events.filter((e) => !id || e.serverId === id),
    player_sessions: () => [
      { id: 3, name: 'Freya', joinedAt: now - 1200_000, leftAt: null, steamId: '76561198000000002' },
      { id: 2, name: 'Bjørn Ironside', joinedAt: now - 3600_000 * 1.2, leftAt: null, steamId: '76561198000000001' },
      { id: 1, name: 'Freya', joinedAt: now - 86400_000, leftAt: now - 86400_000 + 5400_000, steamId: '76561198000000002' },
    ],
    known_players: () => [
      { name: 'Freya', sessions: 7, totalMs: 3600_000 * 11.5, firstSeen: now - 86400_000 * 6, lastSeen: now, steamId: '76561198000000002' },
      { name: 'Bjørn Ironside', sessions: 12, totalMs: 3600_000 * 26, firstSeen: now - 86400_000 * 9, lastSeen: now, steamId: '76561198000000001' },
      { name: 'Griefer', sessions: 2, totalMs: 3600_000 * 1.7, firstSeen: now - 86400_000 * 4, lastSeen: now - 86400_000 * 3, steamId: '76561198000000666' },
    ],
    build_history: () => [{ buildId: '25909431', branch: 'public', ts: now - 3600_000 * 2.3 }],
    create_backup: ({ label }) => {
      const b = { ...backups[0], name: 'new.zip', path: `D:\\x\\${Date.now()}.zip`, createdAt: Date.now(), label: (label as string) || 'Manual backup', kind: 'manual' };
      backups.unshift(b);
      return b;
    },
    list_backups: () => backups,
    get_backup_dir: () => 'D:\\ValheimServers\\Hearth-Hall\\Backups',
    restore_backup: () => 41,
    verify_backup: () => ({ ok: true, files: 44, worlds: ['Midgard', 'Season-1'], hasSettings: true, bytes: 122_400_000, error: null }),
    delete_backup: ({ path }) => {
      const i = backups.findIndex((b) => b.path === path);
      if (i >= 0) backups.splice(i, 1);
    },
    export_pack: () => undefined,
    read_pack: () => ({ format: 1, appVersion: '1.0.0', exportedAt: now - 86400_000, instance: makeInstance('x', 'Friend’s Server', 2456, 'gold'), hasSaves: true, hasConfig: true, includesSecrets: false }),
    import_pack: ({ name }) => {
      const c = makeInstance(`mock-${Date.now()}`, (name as string) || 'Imported', 2468, 'gold');
      instances = [...instances, c];
      status(c.id, { state: 'notInstalled', installedBuild: null });
      return c;
    },
    firewall_state: () => ({ ruleExists: true, portsOk: true, rulePorts: '2456,2457', programRule: true, blockedBy: [] }),
    add_firewall_rules: () => undefined,
    remove_firewall_rules: () => undefined,
    net_info: () => ({ publicIp: '203.0.113.42', local: [{ interface: 'Ethernet', ip: '192.168.1.20' }], gateway: '192.168.1.1' }),
    steam_listing: () => [{ addr: '203.0.113.42:2457', gameport: 2456, appid: 892970, steamid: '90071992547409920' }],
    upnp_probe: () => ({
      found: true,
      gateway: '192.168.1.1',
      routerName: 'Archer AX55',
      manufacturer: 'TP-Link',
      model: 'AX3000',
      externalIp: '203.0.113.42',
      wanKind: 'public',
      mappings: [
        { externalPort: 2456, internalPort: 2456, internalClient: '192.168.1.20', description: 'Valheim Hearth & Hall (2456)', enabled: true },
        { externalPort: 2457, internalPort: 2457, internalClient: '192.168.1.20', description: 'Valheim Hearth & Hall (2457)', enabled: true },
      ],
      error: null,
    }),
    upnp_map: ({ id }) => [`UDP ${find(id).config.port} → 192.168.1.20:${find(id).config.port} forwarded`, `UDP ${find(id).config.port + 1} → 192.168.1.20:${find(id).config.port + 1} forwarded`],
    upnp_unmap: () => undefined,
    run_diagnostics: () => [
      { id: 'install', title: 'Server files', level: 'ok', detail: 'D:\\ValheimServers\\Hearth-Hall\\valheim_server.exe', fix: '' },
      { id: 'settings', title: 'Server settings', level: 'ok', detail: 'Everything the manager can check looks right.', fix: '' },
      { id: 'world', title: 'World', level: 'ok', detail: '“Midgard”, seed Jk5aSQlv4V, day 142 – 64.0 MB.', fix: '' },
      { id: 'admins', title: 'Admins', level: 'ok', detail: '1 admin(s) in adminlist.txt.', fix: '' },
      { id: 'ports', title: 'Ports', level: 'ok', detail: 'UDP 2456 (game) and 2457 (query) are available (used by this server).', fix: '' },
      { id: 'firewall', title: 'Windows Firewall', level: 'warn', detail: 'No inbound rule for this server – players outside your PC may not reach it.', fix: 'firewall' },
      { id: 'mods', title: 'Mods', level: 'ok', detail: 'BepInEx 5.4.2351 with 2 mod(s). Players need the same client-side mods.', fix: '' },
      { id: 'ram', title: 'Memory', level: 'ok', detail: '63.8 GB installed – a server uses about 1.5–4 GB.', fix: '' },
      { id: 'listing', title: 'Steam server list', level: 'ok', detail: 'Listed at 203.0.113.42 – players can find “Hearth & Hall” in the community server list.', fix: '' },
    ],
    disk_usage: () => ({ install: 2.1e9, saves: 180e6, backups: 142e6, logs: 6.2e6 }),
  });

  setInterval(emitTelemetry, 1000);
  let k = 0;
  setInterval(() => {
    for (const i of instances) if (statuses[i.id]?.state === 'running') log(i.id, 'server', LOG_SAMPLES[k++ % LOG_SAMPLES.length]);
  }, 1700);
}
