import { useCallback, useEffect, useMemo, useState } from 'react';
import { ArrowUpCircle, ClipboardList, Download, ExternalLink, FileCode2, FolderOpen, Package, PackageOpen, Puzzle, RefreshCw, ScrollText, Search, Trash2, Upload, Wrench } from 'lucide-react';
import { cn } from '@/lib/cn';
import { api, errMsg, listen } from '@/lib/ipc';
import { ago, bytes, plural } from '@/lib/format';
import { browse, copy, pickFile, reveal } from '@/lib/shell';
import { bepinexDir } from '@/lib/paths';
import type { ModInfo, ModsStatus, ModUpdate, TaskEvent } from '@/lib/types';
import { useApp, useInstance, useStatus } from '@/store/app';
import { Button, IconButton } from '@/components/ui/Button';
import { confirm } from '@/components/ui/Dialog';
import { FieldRow, Input, Toggle } from '@/components/ui/Field';
import { Badge, Callout, Empty, PageHeader, Section } from '@/components/ui/Surface';
import { useDraft } from '../draft';

const THUNDERSTORE = 'https://thunderstore.io/c/valheim/';

function ModRow({ m, update, locked, onToggle, onRemove, onUpdate }: { m: ModInfo; update?: ModUpdate; locked: boolean; onToggle: (on: boolean) => void; onRemove: () => void; onUpdate: () => void }) {
  return (
    <div className={cn('flex items-center gap-3 py-3', !m.enabled && 'opacity-60')}>
      <Toggle size="sm" checked={m.enabled} disabled={locked} onChange={onToggle} />
      <div className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-panel-2 text-fg-3">
        <Package className="size-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2 text-[13px] font-medium text-fg">
          {m.name}
          {m.version && <span className="font-mono text-[11px] font-normal text-fg-4">v{m.version}</span>}
          {m.author && <span className="text-[11.5px] font-normal text-fg-3">by {m.author}</span>}
          {update && <Badge tone="accent">v{update.latest} available</Badge>}
          {!m.enabled && <Badge>disabled</Badge>}
        </div>
        <div className="mt-0.5 truncate text-[11.5px] text-fg-3">{m.description || `${plural(m.dlls, 'plugin DLL')} · ${m.id}`}</div>
        <div className="mt-0.5 font-mono text-[10.5px] text-fg-4">
          {bytes(m.size)} · installed {ago(m.modified)}
          {m.dependencies.length > 0 && ` · needs ${m.dependencies.map((d) => d.split('-').slice(0, 2).join('-')).join(', ')}`}
        </div>
      </div>
      {update && (
        <Button size="sm" variant="primary" icon={<ArrowUpCircle className="size-3.5" />} disabled={locked} onClick={onUpdate}>
          Update
        </Button>
      )}
      {m.package && (
        <IconButton label="Open on Thunderstore" onClick={() => browse(`https://thunderstore.io/c/valheim/p/${m.package!.replace('-', '/')}/`)}>
          <ExternalLink className="size-3.5" />
        </IconButton>
      )}
      <IconButton label="Remove mod" className="hover:text-err" disabled={locked} onClick={onRemove}>
        <Trash2 className="size-3.5" />
      </IconButton>
    </div>
  );
}

export function Mods({ id }: { id: string }) {
  const inst = useInstance(id)!;
  const st = useStatus(id);
  const toast = useApp((s) => s.toast);
  const setView = useApp((s) => s.setView);
  const { draft, patch } = useDraft(id);
  const [s, setS] = useState<ModsStatus | null>(null);
  const [spec, setSpec] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [updates, setUpdates] = useState<ModUpdate[] | null>(null);
  const [log, setLog] = useState<string[] | null>(null);
  const [q, setQ] = useState('');
  const running = !!st && ['running', 'starting', 'stopping', 'restarting'].includes(st.state);
  const installed = st?.state !== 'notInstalled';

  const load = useCallback(() => {
    api.modsStatus(id).then(setS).catch((e) => toast('error', errMsg(e), id));
  }, [id, toast]);
  useEffect(() => {
    load();
    const a = listen<string>('mods-changed', (sid) => sid === id && load());
    // Background installs report through the task stream; refresh when one finishes.
    const b = listen<TaskEvent>('task', (t) => {
      if (t.instanceId === id && t.kind === 'mods' && t.done) {
        setBusy(null);
        load();
      }
    });
    return () => {
      a.then((f) => f());
      b.then((f) => f());
    };
  }, [id, load]);
  useEffect(load, [load, st?.state]);

  const run = async (key: string, fn: () => Promise<unknown>, ok?: string) => {
    setBusy(key);
    try {
      await fn();
      if (ok) toast('success', ok, id);
      load();
    } catch (e) {
      toast('error', errMsg(e), id);
    } finally {
      setBusy(null);
    }
  };
  /** Installs run in the background (downloads); `busy` clears when their task reports done. */
  const startInstall = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast('error', errMsg(e), id);
      setBusy(null);
    }
  };

  const filtered = useMemo(() => {
    const n = q.trim().toLowerCase();
    return (s?.mods ?? []).filter((m) => !n || m.name.toLowerCase().includes(n) || (m.author ?? '').toLowerCase().includes(n) || m.id.toLowerCase().includes(n));
  }, [s, q]);
  const updateFor = (m: ModInfo) => updates?.find((u) => u.id === m.id);
  const enabledCount = s?.mods.filter((m) => m.enabled).length ?? 0;

  return (
    <div className="mx-auto max-w-[1100px] space-y-5 p-6 pb-24">
      <PageHeader
        title="Mods"
        description="Server-side mods through BepInEx – the mod loader nearly every Valheim mod uses – installed from Thunderstore, the community mod site. Mods load when the server starts."
        actions={
          <>
            <Button size="sm" icon={<ExternalLink className="size-3.5" />} onClick={() => browse(THUNDERSTORE)}>
              Browse Thunderstore
            </Button>
            <Button size="sm" icon={<RefreshCw className="size-3.5" />} onClick={load}>
              Refresh
            </Button>
          </>
        }
      />
      {running && <Callout tone="warn">The server has its mods loaded. Stop it to install, update, switch or remove mods – configs can be edited any time (they apply at the next start).</Callout>}
      {!installed && <Callout tone="info">Install the server files first (Install & Files) – BepInEx goes into the server folder.</Callout>}

      <Section
        title={
          <span className="flex items-center gap-2">
            BepInEx {s?.bepinex ? <Badge tone="ok">{s.bepinexVersion ? `v${s.bepinexVersion}` : 'installed'}</Badge> : <Badge>not installed</Badge>}
            {st?.modded && <Badge tone="accent">loaded by the running server</Badge>}
          </span>
        }
        description="denikson-BepInExPack_Valheim – it hooks into valheim_server.exe through a small winhttp.dll next to it."
        icon={<Wrench />}
        actions={
          s?.bepinex ? (
            <>
              <Button size="sm" icon={<FolderOpen className="size-3.5" />} onClick={() => reveal(bepinexDir(inst))}>
                Open BepInEx folder
              </Button>
              <Button size="sm" icon={<RefreshCw className="size-3.5" />} disabled={running || !!busy} loading={busy === 'bepinex'} onClick={() => startInstall('bepinex', () => api.installBepinex(id))}>
                Reinstall / update
              </Button>
              <Button
                size="sm"
                variant="ghost"
                icon={<Trash2 className="size-3.5" />}
                disabled={running || !!busy}
                onClick={async () => {
                  const r = await confirm({
                    title: 'Remove BepInEx?',
                    message: 'BepInEx, its config and every installed mod move to the _mods-removed folder in the server folder (nothing is deleted). The server runs vanilla afterwards.',
                    confirm: 'Remove BepInEx',
                    danger: true,
                  });
                  if (r.ok) run('uninstall', () => api.uninstallBepinex(id), 'BepInEx removed – the server is vanilla again');
                }}
              >
                Remove
              </Button>
            </>
          ) : (
            <Button size="sm" variant="primary" icon={<Download className="size-3.5" />} disabled={running || !installed || !!busy} loading={busy === 'bepinex'} onClick={() => startInstall('bepinex', () => api.installBepinex(id))}>
              Install BepInEx
            </Button>
          )
        }
        bodyClassName="divide-y divide-line"
      >
        {s?.bepinex ? (
          <FieldRow
            label="Load mods"
            description="Off starts the server vanilla without uninstalling anything (the loader is parked as winhttp.dll.disabled). Applies at the next start."
          >
            <Toggle checked={draft.launch.modsEnabled} onChange={(v) => patch((d) => void (d.launch.modsEnabled = v))} />
          </FieldRow>
        ) : (
          <div className="py-3 text-[12px] leading-relaxed text-fg-3">
            Installing a mod from Thunderstore installs BepInEx automatically. Players usually need the same mods (and versions) on their game – except for mods that say they are server-side only.
          </div>
        )}
      </Section>

      <Section title="Install a mod" description="Paste a Thunderstore page URL or the package name (Namespace-Name). Dependencies are installed too." icon={<PackageOpen />} bodyClassName="space-y-3 py-3">
        <div className="flex items-center gap-2">
          <Input
            mono
            value={spec}
            onChange={(e) => setSpec(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && spec.trim() && !running && startInstall('install', async () => (await api.installMod(id, spec), setSpec('')))}
            placeholder="https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/  ·  or  Azumatt-ServerDevcommands"
            className="flex-1"
          />
          <Button
            size="md"
            variant="primary"
            icon={<Download className="size-3.5" />}
            disabled={!spec.trim() || running || !installed || (!!busy && busy !== 'install')}
            loading={busy === 'install'}
            onClick={() => startInstall('install', async () => (await api.installMod(id, spec), setSpec('')))}
          >
            Install
          </Button>
          <Button
            size="md"
            icon={<Upload className="size-3.5" />}
            disabled={running || !s?.bepinex || !!busy}
            onClick={async () => {
              const p = await pickFile('Choose a mod (.zip from Thunderstore, or a plugin .dll)', ['zip', 'dll'], 'Valheim mods');
              if (p) run('file', () => api.installModFile(id, p), 'Mod installed – it loads on the next start');
            }}
          >
            From file…
          </Button>
        </div>
        <div className="text-[11.5px] text-fg-4">Downloads come straight from thunderstore.io. Only install mods you trust – they run with the server’s permissions.</div>
      </Section>

      <Section
        title={
          <span className="flex items-center gap-2">
            Installed mods <Badge tone={enabledCount ? 'accent' : 'neutral'}>{s ? `${enabledCount}/${s.mods.length}` : '…'}</Badge>
          </span>
        }
        description="Everything in BepInEx\plugins. Disabled mods move to plugins_disabled and don’t load."
        icon={<Puzzle />}
        actions={
          <>
            <Button
              size="sm"
              icon={<ArrowUpCircle className="size-3.5" />}
              loading={busy === 'check'}
              disabled={!s?.mods.some((m) => m.package)}
              onClick={() =>
                run('check', async () => {
                  const u = await api.checkModUpdates(id);
                  setUpdates(u);
                  toast(u.length ? 'info' : 'success', u.length ? `${plural(u.length, 'update')} available` : 'All Thunderstore mods are up to date', id);
                })
              }
            >
              Check for updates
            </Button>
            {!!s?.mods.some((m) => m.enabled) && (
              <Button
                size="sm"
                icon={<ClipboardList className="size-3.5" />}
                onClick={() => {
                  const on = s!.mods.filter((m) => m.enabled);
                  const lines = on.map((m) => (m.package ? `${m.package}${m.version ? `-${m.version}` : ''}  https://thunderstore.io/c/valheim/p/${m.package.replace('-', '/')}/` : `${m.name} (installed by hand)`));
                  copy(`Mods on ${inst.config.name} – install the same versions (e.g. with r2modman / Thunderstore Mod Manager):\n${lines.join('\n')}`);
                  toast('success', 'Mod list copied – share it with your players', id);
                }}
              >
                Mod list for players
              </Button>
            )}
            {!!s?.configs.length && (
              <Button size="sm" icon={<FileCode2 className="size-3.5" />} onClick={() => setView(id, 'config')}>
                {plural(s.configs.length, 'config file')}
              </Button>
            )}
          </>
        }
        bodyClassName="py-2"
      >
        {(s?.mods.length ?? 0) > 6 && (
          <div className="relative my-2">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-fg-4" />
            <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter mods" className="pl-8" />
          </div>
        )}
        {!s ? (
          <div className="py-6 text-center text-[12px] text-fg-4">Loading…</div>
        ) : s.mods.length === 0 ? (
          <Empty icon={<Puzzle />} title="No mods installed" description="Install one from Thunderstore above – e.g. a server-side mod like ServerDevcommands, or the mods your group plays with." className="py-8" />
        ) : (
          <div className="divide-y divide-line">
            {filtered.map((m) => (
              <ModRow
                key={m.id}
                m={m}
                update={updateFor(m)}
                locked={running || !!busy}
                onToggle={(on) => run(`t-${m.id}`, () => api.setModEnabled(id, m.id, on), `${m.name} ${on ? 'enabled' : 'disabled'} – applies at the next start`)}
                onUpdate={() => m.package && startInstall('install', async () => {
                  await api.installMod(id, m.package!);
                  setUpdates((u) => u?.filter((x) => x.id !== m.id) ?? null);
                })}
                onRemove={async () => {
                  const r = await confirm({ title: `Remove ${m.name}?`, message: 'It moves to the _mods-removed folder (recoverable). Its config file stays in BepInEx\\config.', confirm: 'Remove', danger: true });
                  if (r.ok) run(`rm-${m.id}`, () => api.removeMod(id, m.id), `${m.name} removed`);
                }}
              />
            ))}
          </div>
        )}
      </Section>

      {s?.bepinex && (
        <Section
          title="BepInEx log"
          description="LogOutput.log from the last modded start – plugin load errors show up here."
          icon={<ScrollText />}
          actions={
            <Button size="sm" icon={<RefreshCw className="size-3.5" />} disabled={!s.hasLog} onClick={() => api.modsLog(id, 200).then(setLog).catch((e) => toast('error', errMsg(e), id))}>
              {log ? 'Reload' : 'Show'}
            </Button>
          }
          bodyClassName="py-3"
        >
          {!s.hasLog ? (
            <div className="text-[12px] text-fg-4">No log yet – it’s written when the server starts with mods on.</div>
          ) : log ? (
            <div className="selectable max-h-[320px] overflow-auto rounded-lg bg-bg/70 p-3 font-mono text-[11px] leading-[1.6]">
              {log.map((l, i) => (
                <div key={i} className={cn('whitespace-pre-wrap', /^\[(Error|Fatal)/.test(l) ? 'text-err' : /^\[Warning/.test(l) ? 'text-warn' : /^\[Message/.test(l) ? 'text-fg-2' : 'text-fg-3')}>
                  {l}
                </div>
              ))}
            </div>
          ) : (
            <div className="text-[12px] text-fg-4">Click Show to load the last 200 lines.</div>
          )}
        </Section>
      )}
    </div>
  );
}
