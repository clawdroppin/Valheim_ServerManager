// Shared network helpers and a short-lived cache for slow lookups.
import { api } from './ipc';
import type { NetInfo, ServerInstance } from './types';

export interface PortRow {
  port: number;
  label: string;
  purpose: string;
  required: boolean;
  protocol: 'UDP' | 'TCP';
}

/** Every port the server binds (verified with netstat: UDP port and port + 1). With Crossplay the
 * game traffic goes through PlayFab's relay, so forwarding is optional. */
export function serverPorts(inst: ServerInstance): PortRow[] {
  const cp = inst.config.crossplay;
  return [
    { port: inst.config.port, label: 'Game', purpose: cp ? 'Steam traffic – Crossplay works without forwarding' : 'Player traffic – forward it so players can connect', required: !cp, protocol: 'UDP' },
    { port: inst.config.port + 1, label: 'Query', purpose: 'Steam server list and player count', required: !cp, protocol: 'UDP' },
  ];
}

let netCache: { at: number; value: Promise<NetInfo> } | null = null;

/** Public IP + LAN addresses + gateway, cached for 5 minutes (the public IP lookup is an HTTP call). */
export function netInfo(force = false): Promise<NetInfo> {
  if (!force && netCache && Date.now() - netCache.at < 300_000) return netCache.value;
  const value = api.netInfo().catch((e) => {
    netCache = null;
    throw e;
  });
  netCache = { at: Date.now(), value };
  return value;
}

/** The LAN address players' traffic should be forwarded to. */
export function lanAddress(_inst: ServerInstance, net: NetInfo | null): string | null {
  return net?.local[0]?.ip ?? null;
}
