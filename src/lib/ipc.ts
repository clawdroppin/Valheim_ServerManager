import { invoke as tauriInvoke, isTauri } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import type * as T from './types';

type Handler = (args: Record<string, unknown>) => unknown | Promise<unknown>;
type Mock = { handlers: Record<string, Handler>; listeners: Map<string, Set<(p: unknown) => void>> };

let mock: Mock | null = null;

/** Browser preview only (`npm run dev` outside Tauri) – see devMock.ts. */
export function installMock(handlers: Record<string, Handler>) {
  mock = { handlers, listeners: new Map() };
}
export function mockEmit(event: string, payload: unknown) {
  mock?.listeners.get(event)?.forEach((fn) => fn(payload));
}

async function invoke<R>(cmd: string, args: Record<string, unknown> = {}): Promise<R> {
  if (mock) {
    const h = mock.handlers[cmd];
    if (!h) throw `Mock: command ${cmd} not implemented`;
    await new Promise((r) => setTimeout(r, 40));
    return (await h(args)) as R;
  }
  return tauriInvoke<R>(cmd, args);
}

export async function listen<P>(event: string, fn: (payload: P) => void): Promise<() => void> {
  if (mock) {
    const set = mock.listeners.get(event) ?? new Set();
    mock.listeners.set(event, set);
    const cb = (p: unknown) => fn(p as P);
    set.add(cb);
    return () => set.delete(cb);
  }
  return tauriListen<P>(event, (e) => fn(e.payload));
}

export const inTauri = () => isTauri();

/** Typed wrappers over every Rust command. Errors arrive as plain strings. */
export const api = {
  appInfo: () => invoke<T.AppInfo>('get_app_info'),
  getSettings: () => invoke<T.AppSettings>('get_settings'),
  saveSettings: (settings: T.AppSettings) => invoke<void>('save_settings', { settings }),
  cancelTask: (taskId: string) => invoke<void>('cancel_task', { taskId }),
  testWebhook: (url: string) => invoke<void>('test_webhook', { url }),
  quit: () => invoke<void>('app_quit'),
  hideWindow: () => invoke<void>('window_hide'),
  probeFolder: (path: string) => invoke<T.FolderProbe>('probe_folder', { path }),

  listInstances: () => invoke<T.ServerInstance[]>('list_instances'),
  getStatuses: () => invoke<T.ServerStatus[]>('get_statuses'),
  suggestPorts: (excludeId?: string) => invoke<T.PortSet>('suggest_ports', { excludeId: excludeId ?? null }),
  createInstance: (input: { name: string; installDir?: string | null; color?: string; config: T.ServerConfig; installNow: boolean; templateId?: string | null }) =>
    invoke<T.ServerInstance>('create_instance', { input }),
  updateInstance: (instance: T.ServerInstance) => invoke<T.ServerInstance>('update_instance', { instance }),
  validateConfig: (config: T.ServerConfig) => invoke<T.ConfigIssue[]>('validate_config', { config }),
  reorderInstances: (ids: string[]) => invoke<void>('reorder_instances', { ids }),
  deleteInstance: (id: string, deleteFiles: boolean) => invoke<void>('delete_instance', { id, deleteFiles }),
  cloneInstance: (id: string, name: string, copySaves: boolean) => invoke<T.ServerInstance>('clone_instance', { id, name, copySaves }),
  findExistingInstalls: () => invoke<string[]>('find_existing_installs'),
  importExisting: (path: string, name?: string) => invoke<T.ServerInstance>('import_existing', { path, name: name ?? null }),

  installServer: (id: string, validate = false) => invoke<void>('install_server', { id, validate }),
  checkUpdate: (id: string) => invoke<boolean>('check_update', { id }),
  start: (id: string) => invoke<void>('start_server', { id }),
  stop: (id: string, force = false) => invoke<void>('stop_server', { id, force }),
  restart: (id: string, update = false) => invoke<void>('restart_server', { id, update }),
  cancelDeferred: (id: string) => invoke<void>('cancel_deferred', { id }),
  launchLine: (id: string) => invoke<string>('get_launch_line', { id }),
  setConsoleVisible: (id: string, visible: boolean) => invoke<number>('set_console_visible', { id, visible }),
  clearCrashState: (id: string) => invoke<void>('clear_crash_state', { id }),
  queryServer: (id: string, address?: string) => invoke<T.QueryResult>('query_server', { id, address: address ?? null }),

  readConfig: (id: string, file: T.ConfigFileKind) => invoke<T.ConfigFile>('read_config', { id, file }),
  writeConfig: (id: string, file: T.ConfigFileKind, text: string) => invoke<{ restartNeeded: boolean; instance: T.ServerInstance }>('write_config', { id, file, text }),
  configHistory: (id: string, file: T.ConfigFileKind) => invoke<[string, number][]>('list_config_history', { id, file }),
  readTextFile: (id: string, path: string) => invoke<string>('read_text_file', { id, path }),
  listRulePresets: () => invoke<T.RulePreset[]>('list_rule_presets'),
  saveRulePreset: (name: string, settings: T.RulePreset['settings']) => invoke<T.RulePreset>('save_rule_preset', { name, settings }),
  deleteRulePreset: (name: string) => invoke<void>('delete_rule_preset', { name }),
  getAccess: (id: string) => invoke<T.AccessView>('get_access', { id }),
  setAccess: (id: string, list: T.AccessList, playerId: string, add: boolean) => invoke<boolean>('set_access', { id, list, playerId, add }),

  logTail: (id: string, maxLines: number) => invoke<string[]>('read_log_tail', { id, maxLines }),
  logFiles: (id: string) => invoke<T.LogFile[]>('list_log_files', { id }),

  listWorlds: (id: string) => invoke<T.World[]>('list_worlds', { id }),
  listLocalWorlds: () => invoke<T.LocalWorld[]>('list_local_worlds'),
  scanWorldFolder: (path: string) => invoke<T.World[]>('scan_world_folder', { path }),
  importWorld: (id: string, path: string, name: string, activate: boolean) => invoke<{ name: string; files: number }>('import_world', { id, path, name, activate }),
  restoreWorldBackup: (id: string, world: string, backup: string) => invoke<string>('restore_world_backup', { id, world, backup }),
  exportWorld: (id: string, world: string, dest: string) => invoke<number>('export_world', { id, world, dest }),
  duplicateWorld: (id: string, world: string, newName: string) => invoke<string>('duplicate_world', { id, world, newName }),
  deleteWorld: (id: string, world: string) => invoke<void>('delete_world', { id, world }),
  resetWorld: (id: string) => invoke<string | null>('reset_world', { id }),
  activateWorld: (id: string, name: string) => invoke<T.ServerInstance>('activate_world', { id, name }),
  renameWorld: (id: string, world: string, newName: string) => invoke<string>('rename_world', { id, world, newName }),
  checkWorld: (id: string, world: string) => invoke<T.SaveCheck[]>('check_world', { id, world }),

  modsStatus: (id: string) => invoke<T.ModsStatus>('mods_status', { id }),
  installBepinex: (id: string) => invoke<void>('install_bepinex', { id }),
  uninstallBepinex: (id: string) => invoke<string>('uninstall_bepinex', { id }),
  installMod: (id: string, spec: string) => invoke<void>('install_mod', { id, spec }),
  installModFile: (id: string, path: string) => invoke<string>('install_mod_file', { id, path }),
  setModEnabled: (id: string, modId: string, enabled: boolean) => invoke<void>('set_mod_enabled', { id, modId, enabled }),
  removeMod: (id: string, modId: string) => invoke<void>('remove_mod', { id, modId }),
  checkModUpdates: (id: string) => invoke<T.ModUpdate[]>('check_mod_updates', { id }),
  readModConfig: (id: string, name: string) => invoke<T.ConfigFile>('read_mod_config', { id, name }),
  writeModConfig: (id: string, name: string, text: string) => invoke<boolean>('write_mod_config', { id, name, text }),
  modConfigHistory: (id: string, name: string) => invoke<[string, number][]>('mod_config_history', { id, name }),
  modsLog: (id: string, lines: number) => invoke<string[]>('mods_log', { id, lines }),

  metricsHistory: (id: string, since: number) => invoke<T.MetricRow[]>('metrics_history', { id, since }),
  eventHistory: (id: string | null, limit: number) => invoke<T.EventRow[]>('event_history', { id, limit }),
  playerSessions: (id: string, limit: number) => invoke<T.SessionRow[]>('player_sessions', { id, limit }),
  knownPlayers: (id: string) => invoke<T.PlayerSummary[]>('known_players', { id }),
  buildHistory: (id: string) => invoke<T.BuildRow[]>('build_history', { id }),

  createBackup: (id: string, label: string) => invoke<T.BackupInfo>('create_backup', { id, label }),
  listBackups: (id: string) => invoke<T.BackupInfo[]>('list_backups', { id }),
  backupDir: (id: string) => invoke<string>('get_backup_dir', { id }),
  restoreBackup: (id: string, path: string, includeConfig: boolean) => invoke<number>('restore_backup', { id, path, includeConfig }),
  deleteBackup: (id: string, path: string) => invoke<void>('delete_backup', { id, path }),
  verifyBackup: (id: string, path: string) => invoke<T.VerifyResult>('verify_backup', { id, path }),

  exportPack: (id: string, dest: string, options: { includeSaves: boolean; includeSecrets: boolean }) => invoke<void>('export_pack', { id, dest, options }),
  readPack: (path: string) => invoke<T.PackManifest>('read_pack', { path }),
  importPack: (path: string, name: string, installDir: string | null, installNow: boolean) => invoke<T.ServerInstance>('import_pack', { path, name, installDir, installNow }),

  firewallState: (id: string) => invoke<T.FirewallState>('firewall_state', { id }),
  addFirewallRules: (id: string) => invoke<void>('add_firewall_rules', { id }),
  removeFirewallRules: (id: string) => invoke<void>('remove_firewall_rules', { id }),
  netInfo: () => invoke<T.NetInfo>('net_info'),
  steamListing: (id: string) => invoke<T.SteamListing[]>('steam_listing', { id }),
  upnpProbe: () => invoke<T.UpnpInfo>('upnp_probe'),
  upnpMap: (id: string) => invoke<string[]>('upnp_map', { id }),
  upnpUnmap: (id: string) => invoke<void>('upnp_unmap', { id }),

  diagnostics: (id: string) => invoke<T.DiagCheck[]>('run_diagnostics', { id }),
  diskUsage: (id: string) => invoke<T.DiskUsage>('disk_usage', { id }),
};

export function errMsg(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
