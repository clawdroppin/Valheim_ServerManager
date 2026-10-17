import { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, ArrowRightLeft, Check, ChevronDown, ChevronRight, Copy, CopyPlus, FolderOpen, FolderSearch, Globe2, History, Laptop, Map as MapIcon, PenLine, Play, Plus, RefreshCw, RotateCcw, ShieldCheck, Sparkles, Sprout, Sun, Trash2, Upload } from 'lucide-react';
import { api, errMsg } from '@/lib/ipc';
import { ago, bytes, dateTime, plural } from '@/lib/format';
import { browse, copy, pickFolder, reveal } from '@/lib/shell';
import { worldsDir } from '@/lib/paths';
import { describeWorldKeys } from '@/data/gameplay';
import type { LocalWorld, SaveCheck, World, WorldBackup } from '@/lib/types';
import { useApp, useInstance, useStatus } from '@/store/app';
import { Button } from '@/components/ui/Button';
import { confirm } from '@/components/ui/Dialog';
import { Input } from '@/components/ui/Field';
import { Badge, Callout, Card, Empty, PageHeader, Section } from '@/components/ui/Surface';
import { cn } from '@/lib/cn';
import { discardDraft } from '../draft';

const KIND_LABEL: Record<string, string> = { auto: 'Automatic', restore: 'Before a restore', cloud: 'Cloud', manual: 'Manual' };

function CheckBadge({ check }: { check?: SaveCheck }) {
  if (!check) return null;
  return check.ok ? (
    <Badge tone="ok" title={`${bytes(check.bytes)} of world data`}>
      <ShieldCheck /> Intact
    </Badge>
  ) : (
    <Badge tone="err" title={check.error ?? ''}>
      <AlertTriangle /> {check.error ?? 'Damaged'}
    </Badge>
  );
}

function BackupRow({ b, check, onRestore, disabled }: { b: WorldBackup; check?: SaveCheck; onRestore: () => void; disabled: boolean }) {
  return (
    <div className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-panel-2">
      <History className="size-4 shrink-0 text-fg-4" />
      <div className="min-w-0 flex-1">
        <div className="text-[12.5px] text-fg">
          {dateTime(b.createdAt)} <span className="text-fg-4">· {ago(b.createdAt)}</span>
        </div>
        <div className="truncate font-mono text-[10.5px] text-fg-4">
          {b.name} · {KIND_LABEL[b.kind] ?? b.kind} · {bytes(b.size)}
        </div>
      </div>
      <CheckBadge check={check} />
      <Button size="sm" variant="outline" icon={<RotateCcw className="size-3.5" />} disabled={disabled} onClick={onRestore}>
        Roll back to this
      </Button>
    </div>
  );
}

function WorldFacts({ w }: { w: World }) {
  const mods = w.meta ? describeWorldKeys(w.meta.keys) : [];
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {w.meta && (
        <button onClick={() => copy(w.meta!.seedName)} title="Copy the seed" className="flex items-center gap-1 rounded-md bg-panel-2 px-2 py-0.5 font-mono text-[11px] text-fg-2 hover:bg-hover">
          <Sprout className="size-3 text-ok" /> {w.meta.seedName} <Copy className="size-3 text-fg-4" />
        </button>
      )}
      {w.meta?.seedName && (
        <button
          onClick={() => browse(`https://valheim-map.world/?seed=${encodeURIComponent(w.meta!.seedName)}`)}
          title="Open this seed in the community map viewer (valheim-map.world)"
          className="flex items-center gap-1 rounded-md bg-panel-2 px-2 py-0.5 text-[11px] text-fg-2 hover:bg-hover"
        >
          <MapIcon className="size-3 text-info" /> Map
        </button>
      )}
      {w.day != null && (
        <Badge>
          <Sun /> Day {w.day}
        </Badge>
      )}
      {w.format === 'legacy' && <Badge tone="warn">Old save format – converted on first load</Badge>}
      {mods.map((m) => (
        <Badge key={m} tone="accent">
          {m}
        </Badge>
      ))}
    </div>
  );
}

export function Worlds({ id }: { id: string }) {
  const inst = useInstance(id)!;
  const st = useStatus(id);
  const toast = useApp((s) => s.toast);
  const upsert = useApp((s) => s.upsertInstance);
  const confirmOn = useApp((s) => s.settings?.confirmDestructive ?? true);
  const [worlds, setWorlds] = useState<World[] | null>(null);
  const [local, setLocal] = useState<LocalWorld[] | null>(null);
  const [picked, setPicked] = useState<{ dir: string; worlds: World[] } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [newName, setNewName] = useState('');
  const [checks, setChecks] = useState<Record<string, SaveCheck>>({});
  const running = st?.state === 'running' || st?.state === 'starting' || st?.state === 'stopping';

  const load = useCallback(() => {
    api.listWorlds(id).then(setWorlds).catch((e) => toast('error', errMsg(e)));
    api.listLocalWorlds().then(setLocal).catch(() => setLocal([]));
  }, [id, toast]);
  useEffect(load, [load, st?.state, st?.lastSave]);

  const active = worlds?.find((w) => w.active);
  const others = worlds?.filter((w) => !w.active) ?? [];

  const run = async (key: string, fn: () => Promise<unknown>, ok: string) => {
    setBusy(key);
    try {
      await fn();
      toast('success', ok, id);
      load();
    } catch (e) {
      toast('error', errMsg(e), id);
    } finally {
      setBusy(null);
    }
  };
  const activate = (name: string) =>
    run(
      `act-${name}`,
      async () => {
        const out = await api.activateWorld(id, name);
        discardDraft(id);
        upsert(out);
      },
      running ? `“${name}” loads at the next restart` : `“${name}” is now the server world`,
    );

  const rename = async (w: World) => {
    const r = await confirm({
      title: `Rename world “${w.name}”`,
      message: w.active
        ? 'The world is copied under the new name (its name inside the save changes too), the old copy goes to quarantine, and the server keeps loading it.'
        : 'The world is copied under the new name (its name inside the save changes too) and the old copy goes to quarantine.',
      input: w.name,
      inputPlaceholder: 'New world name',
      confirm: 'Rename',
    });
    if (!r.ok || r.value === w.name) return;
    await run(
      `ren-${w.name}`,
      async () => {
        const name = await api.renameWorld(id, w.name, r.value);
        if (w.active) {
          discardDraft(id);
          upsert({ ...inst, config: { ...inst.config, world: name } });
        }
      },
      `Renamed to “${r.value}”`,
    );
  };

  const duplicate = async (w: World) => {
    const r = await confirm({ title: `Duplicate “${w.name}”`, message: 'A full copy under a new name – the original is untouched.', input: `${w.name}-copy`, inputPlaceholder: 'Name of the copy', confirm: 'Duplicate' });
    if (r.ok) run(`dup-${w.name}`, () => api.duplicateWorld(id, w.name, r.value), 'World copied');
  };

  const checkSaves = async (w: World) => {
    setBusy('check');
    try {
      const res = await api.checkWorld(id, w.name);
      setChecks(Object.fromEntries(res.map((c) => [c.file, c])));
      const bad = res.filter((c) => !c.ok);
      if (!bad.length) toast('success', `The save and ${plural(res.length - 1, 'backup')} are intact`, id);
      else if (bad[0].file === 'Current save') toast('error', `The current save is damaged (${bad[0].error}) – roll back to an intact backup before starting`, id);
      else toast('warn', `${plural(bad.length, 'backup')} damaged – the current save is fine`, id);
    } catch (e) {
      toast('error', errMsg(e), id);
    } finally {
      setBusy(null);
    }
  };

  const importWorld = async (w: World, label: string) => {
    const useNow = !running;
    await run(
      `imp-${w.path}`,
      () => api.importWorld(id, w.path, w.name, useNow),
      useNow ? `Imported “${label}” – it loads on the next start` : `Imported “${label}” – switch to it after stopping the server`,
    );
    setPicked(null);
  };

  return (
    <div className="mx-auto max-w-[1100px] space-y-5 p-6">
      <PageHeader
        title="Worlds"
        description="Each world is a folder in worlds_local. The server loads the one named under Server Settings, saves it on its save interval and on every stop, and keeps its own backups next to it."
        actions={
          <>
            <Button size="sm" icon={<FolderOpen className="size-3.5" />} onClick={() => reveal(worldsDir(inst))}>
              Open worlds folder
            </Button>
            <Button size="sm" icon={<RefreshCw className="size-3.5" />} onClick={load}>
              Refresh
            </Button>
          </>
        }
      />
      {running && <Callout tone="warn">The server has its world open. Stop it to roll back, rename, reset or delete the active world – switching, exporting and backups work while it runs (a switch applies at the next restart).</Callout>}

      <Section
        title={
          <span className="flex items-center gap-2">
            Active world <span className="font-mono text-[11.5px] font-normal text-fg-4">{inst.config.world}</span>
          </span>
        }
        description={
          active
            ? `Last saved ${ago(active.savedAt)} · ${bytes(active.size)}${active.saveNumber != null ? ` · save #${active.saveNumber}` : ''} · ${plural(active.backups.length, 'server backup')}`
            : 'Not created yet – the server generates it on the next start.'
        }
        icon={<Globe2 />}
        actions={
          active && (
            <>
              <Button
                size="sm"
                icon={<Upload className="size-3.5" />}
                onClick={async () => {
                  const dest = await pickFolder('Export the world to… (e.g. your game’s worlds_local folder)');
                  if (dest) run('export', () => api.exportWorld(id, active.name, dest), 'World exported');
                }}
              >
                Export
              </Button>
              <Button size="sm" icon={<ShieldCheck className="size-3.5" />} loading={busy === 'check'} onClick={() => checkSaves(active)}>
                Check saves
              </Button>
              <Button size="sm" icon={<CopyPlus className="size-3.5" />} onClick={() => duplicate(active)}>
                Duplicate
              </Button>
              <Button size="sm" icon={<PenLine className="size-3.5" />} disabled={running} onClick={() => rename(active)}>
                Rename
              </Button>
              <Button
                size="sm"
                variant="danger"
                icon={<Sparkles className="size-3.5" />}
                disabled={running}
                loading={busy === 'reset'}
                onClick={async () => {
                  if (confirmOn) {
                    const r = await confirm({
                      title: 'Reset this world?',
                      message: 'A backup is made and the world moves to the quarantine folder (nothing is deleted). The next start generates a brand-new world with a new seed under the same name. Characters are kept – they live on the players’ PCs.',
                      confirm: 'Reset world',
                      danger: true,
                    });
                    if (!r.ok) return;
                  }
                  run('reset', () => api.resetWorld(id), 'World reset – a new one is generated on the next start');
                }}
              >
                Reset
              </Button>
            </>
          )
        }
        bodyClassName="py-3 space-y-3"
      >
        {!worlds ? (
          <div className="py-6 text-center text-[12px] text-fg-4">Loading…</div>
        ) : !active ? (
          <Empty icon={<Globe2 />} title="No world yet" description="Start the server to generate it, or bring one over from the game below." className="py-6" />
        ) : (
          <>
            <div className="flex items-center gap-3">
              <WorldFacts w={active} />
              <span className="ml-auto">
                <CheckBadge check={checks['Current save']} />
              </span>
            </div>
            <div className="space-y-1">
              <div className="mb-1 flex items-center gap-2 text-[11px] tracking-wide text-fg-4 uppercase">
                <History className="size-3.5" /> The server’s own backups, newest first
              </div>
              {active.backups.length === 0 ? (
                <div className="rounded-lg px-3 py-2 text-[12px] text-fg-4">
                  None yet – the server makes its first one after {Math.round(inst.config.backupShort / 3600)} h of play (Server Settings → Server backups). The manager’s zip backups are under Backups.
                </div>
              ) : (
                active.backups.map((b) => (
                  <BackupRow
                    key={b.name}
                    b={b}
                    check={checks[b.name]}
                    disabled={running || busy != null}
                    onRestore={async () => {
                      const r = await confirm({
                        title: `Roll back to the backup from ${dateTime(b.createdAt)}?`,
                        message: 'A zip backup is made first and the current world moves to quarantine, then this backup becomes the world. Everything built after it is gone from the world.',
                        confirm: 'Roll back',
                        danger: true,
                      });
                      if (r.ok) run(`bk-${b.name}`, () => api.restoreWorldBackup(id, active.name, b.name), 'Rolled back – the server loads that backup next');
                    }}
                  />
                ))
              )}
            </div>
          </>
        )}
      </Section>

      <Section title="Other worlds" description="Worlds kept on this server. Switch the server to one, or start a brand-new world under a new name." icon={<Globe2 />} bodyClassName="py-3 space-y-3">
        <div className="flex items-center gap-2">
          <Input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="New world name – e.g. Midgard-2" maxLength={64} className="max-w-[320px]" />
          <Button
            size="md"
            icon={<Plus className="size-3.5" />}
            disabled={!newName.trim() || /[\\/:*?"<>|]/.test(newName) || newName.includes('_backup_')}
            onClick={async () => {
              const r = await confirm({ title: `Start a new world “${newName.trim()}”?`, message: 'The server switches to it and generates it (with a random seed) on the next start. Your current world stays where it is – switch back any time.', confirm: 'New world' });
              if (r.ok) {
                await activate(newName.trim());
                setNewName('');
              }
            }}
          >
            Start new world
          </Button>
        </div>
        {others.length === 0 ? (
          <div className="py-3 text-center text-[12px] text-fg-4">No other worlds.</div>
        ) : (
          <div className="divide-y divide-line">
            {others.map((w) => {
              const expanded = open === w.name;
              return (
                <div key={w.name} className="py-2">
                  <div className="flex items-center gap-3">
                    <button onClick={() => setOpen(expanded ? null : w.name)} className="flex size-6 items-center justify-center rounded text-fg-3 hover:bg-hover">
                      {expanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
                    </button>
                    <span className="font-medium text-fg">{w.name}</span>
                    <span className="text-[12px] text-fg-3">
                      saved {ago(w.savedAt)} · {bytes(w.size)}
                      {w.day != null && ` · day ${w.day}`}
                    </span>
                    <div className="ml-auto flex gap-1.5">
                      <Button size="sm" variant="primary" icon={<Play className="size-3.5" />} loading={busy === `act-${w.name}`} onClick={() => activate(w.name)}>
                        Use this world
                      </Button>
                      <Button
                        size="sm"
                        icon={<Upload className="size-3.5" />}
                        onClick={async () => {
                          const dest = await pickFolder('Export the world to…');
                          if (dest) run(`exp-${w.name}`, () => api.exportWorld(id, w.name, dest), 'World exported');
                        }}
                      >
                        Export
                      </Button>
                      <Button size="sm" icon={<PenLine className="size-3.5" />} onClick={() => rename(w)}>
                        Rename
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<Trash2 className="size-3.5" />}
                        onClick={async () => {
                          const r = await confirm({ title: `Remove world “${w.name}”?`, message: 'It moves to the _quarantine folder next to worlds_local – delete it from there yourself once you are sure.', confirm: 'Remove', danger: true });
                          if (r.ok) run(`del-${w.name}`, () => api.deleteWorld(id, w.name), 'Moved to quarantine');
                        }}
                      >
                        Remove
                      </Button>
                    </div>
                  </div>
                  {expanded && (
                    <div className={cn('mt-2 ml-9 space-y-1.5')}>
                      <WorldFacts w={w} />
                      <div className="text-[11.5px] text-fg-4">{plural(w.backups.length, 'server backup')} · {w.path}</div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </Section>

      <Section
        title="Bring a world from the game"
        description="Copy a world you played in single player or hosted from the game (or from another server) onto this one. The original stays untouched."
        icon={<ArrowRightLeft />}
        actions={
          <Button
            size="sm"
            icon={<FolderSearch className="size-3.5" />}
            onClick={async () => {
              const dir = await pickFolder('Pick a worlds_local folder, a world folder, or a folder with .fwl files');
              if (!dir) return;
              try {
                const ws = await api.scanWorldFolder(dir);
                if (!ws.length) toast('warn', 'No Valheim worlds found there (world folders with _main.N.fwl2, or .fwl + .db files).', id);
                setPicked({ dir, worlds: ws });
              } catch (e) {
                toast('error', errMsg(e), id);
              }
            }}
          >
            From another folder…
          </Button>
        }
        bodyClassName="py-3"
      >
        {picked && (
          <div className="mb-3 space-y-1.5 rounded-xl border border-accent/30 bg-accent/[0.04] p-3">
            <div className="truncate font-mono text-[11px] text-fg-3">{picked.dir}</div>
            {picked.worlds.map((w) => (
              <div key={w.path} className="flex items-center gap-3 rounded-lg px-2 py-1.5">
                <Globe2 className="size-4 text-fg-3" />
                <span className="font-mono text-[12.5px] text-fg">{w.name}</span>
                <span className="text-[12px] text-fg-3">
                  saved {ago(w.savedAt)} · {bytes(w.size)}
                  {w.meta && ` · seed ${w.meta.seedName}`}
                </span>
                <Button size="sm" variant="primary" className="ml-auto" loading={busy === `imp-${w.path}`} onClick={() => importWorld(w, w.name)}>
                  Import
                </Button>
              </div>
            ))}
          </div>
        )}
        {!local ? (
          <div className="py-5 text-center text-[12px] text-fg-4">Looking for game worlds…</div>
        ) : local.length === 0 ? (
          <div className="flex items-center gap-3 py-3 text-[12px] text-fg-3">
            <Laptop className="size-4 shrink-0 text-fg-4" />
            No Valheim worlds on this PC. Looked in AppData\LocalLow\IronGate\Valheim\worlds_local and Steam Cloud (userdata\…\892970). Copy a world from another PC and use “From another folder”.
          </div>
        ) : (
          <div className="divide-y divide-line">
            {local.map((l) => (
              <div key={l.world.path} className="flex items-center gap-3 py-2.5">
                <Laptop className="size-4 shrink-0 text-fg-3" />
                <div className="min-w-0 flex-1">
                  <div className="text-[12.5px] text-fg">
                    <span className="font-mono">{l.world.name}</span> <span className="text-fg-3">· saved {ago(l.world.savedAt)}</span>
                  </div>
                  <div className="truncate text-[11px] text-fg-4">
                    {l.source} · {bytes(l.world.size)}
                    {l.world.meta && ` · seed ${l.world.meta.seedName}`}
                    {l.world.day != null && ` · day ${l.world.day}`}
                  </div>
                </div>
                <Button size="sm" variant="primary" loading={busy === `imp-${l.world.path}`} onClick={() => importWorld(l.world, l.world.name)}>
                  Import
                </Button>
              </div>
            ))}
          </div>
        )}
      </Section>

      <Card className="p-4 text-[12px] leading-relaxed text-fg-3">
        <b className="text-fg-2">Characters travel with the players.</b> Valheim stores characters on each player’s PC – the server only holds the world (terrain, buildings, chests, portals). Switching or
        resetting a world never touches anyone’s character, inventory or skills. <Check className="inline size-3.5 text-ok" /> Rollbacks, renames, resets and imports always keep a copy first.
      </Card>
    </div>
  );
}
