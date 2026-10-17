import type { ServerConfig } from './types';

/** A fresh profile's command-line settings (mirrors ServerConfig::default in models.rs). */
export function defaultConfig(over: Partial<ServerConfig> = {}): ServerConfig {
  return {
    name: 'Valheim Server',
    port: 2456,
    world: 'Dedicated',
    password: '',
    public: true,
    crossplay: false,
    instanceId: '',
    saveInterval: 1800,
    backups: 4,
    backupShort: 7200,
    backupLong: 43200,
    preset: '',
    modifiers: {},
    keys: [],
    ...over,
  };
}
