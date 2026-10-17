import type { ServerInstance } from './types';

/** `-savedir` resolved like models.rs `ServerInstance::data_dir` (relative = inside the install folder). */
export function dataDir(inst: ServerInstance): string {
  const d = (inst.launch.dataDir || '').trim() || './save-data';
  if (/^[a-zA-Z]:[\\/]/.test(d) || d.startsWith('\\\\')) return d.replace(/\//g, '\\');
  return `${inst.installDir.replace(/[\\/]+$/, '')}\\${d.replace(/^\.[\\/]/, '').replace(/\//g, '\\')}`;
}

export const worldsDir = (inst: ServerInstance) => `${dataDir(inst)}\\worlds_local`;
export const bepinexDir = (inst: ServerInstance) => `${inst.installDir.replace(/[\\/]+$/, '')}\\BepInEx`;
