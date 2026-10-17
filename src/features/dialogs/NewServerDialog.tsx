import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ArrowLeft, ArrowRight, Check, FolderOpen, Globe2, Rocket, Server, Sparkles } from 'lucide-react';
import { cn } from '@/lib/cn';
import { api, errMsg } from '@/lib/ipc';
import { bytes } from '@/lib/format';
import { SERVER_COLORS } from '@/lib/colors';
import { pickFolder } from '@/lib/shell';
import { defaultConfig } from '@/lib/defaults';
import { PRESETS, presetLabel } from '@/data/gameplay';
import type { FolderProbe } from '@/lib/types';
import { useApp } from '@/store/app';
import { Button } from '@/components/ui/Button';
import { Dialog } from '@/components/ui/Dialog';
import { Checkbox, Input, Label, NumberInput, SecretInput, Select, Toggle } from '@/components/ui/Field';
import { Callout } from '@/components/ui/Surface';
import { passwordProblem } from '@/features/server/views/Settings';
import { isPlayerId } from '@/features/server/views/Access';

const STEPS = [
  { title: 'Server', icon: Server },
  { title: 'World & access', icon: Globe2 },
  { title: 'Difficulty', icon: Sparkles },
  { title: 'Launch', icon: Rocket },
];

const badWorld = (w: string) => !w.trim() || /[\\/:*?"<>|]/.test(w) || w.startsWith('.') || w.includes('_backup_');

export function NewServerDialog() {
  const dialog = useApp((s) => s.dialog);
  const setDialog = useApp((s) => s.setDialog);
  const instances = useApp((s) => s.instances);
  const open = dialog?.kind === 'new';
  const [step, setStep] = useState(0);
  const [name, setName] = useState('');
  const [serverName, setServerName] = useState('');
  const [color, setColor] = useState('fjord');
  const [dir, setDir] = useState('');
  const [port, setPort] = useState(2456);
  const [world, setWorld] = useState('Dedicated');
  const [password, setPassword] = useState('');
  const [listed, setListed] = useState(true);
  const [crossplay, setCrossplay] = useState(false);
  const [preset, setPreset] = useState('');
  const [adminId, setAdminId] = useState('');
  const [template, setTemplate] = useState('');
  const [installNow, setInstallNow] = useState(true);
  const [probe, setProbe] = useState<FolderProbe | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const n = instances.length + 1;
    setStep(0);
    setName(n === 1 ? 'My Valheim Server' : `Valheim Server ${n}`);
    setServerName('');
    setColor(Object.keys(SERVER_COLORS)[instances.length % Object.keys(SERVER_COLORS).length]);
    setDir('');
    setTemplate(dialog?.kind === 'new' ? (dialog.templateId ?? '') : '');
    setWorld('Dedicated');
    setPassword('');
    setListed(true);
    setCrossplay(false);
    setPreset('');
    setAdminId('');
    setInstallNow(true);
    setError(null);
    api.suggestPorts().then((p) => setPort(p.port)).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!dir) return setProbe(null);
    const t = setTimeout(() => api.probeFolder(dir).then(setProbe).catch(() => setProbe(null)), 250);
    return () => clearTimeout(t);
  }, [dir]);

  const listName = serverName.trim() || name.trim();
  const portClash = instances.find((i) => [i.config.port, i.config.port + 1].some((p) => p === port || p === port + 1));
  const pwProblem = passwordProblem(listName, password, listed);
  const canNext =
    step === 0 ? name.trim().length > 0 && !portClash : step === 1 ? listName.length > 0 && !listName.includes('"') && !pwProblem && !badWorld(world) && (!adminId || isPlayerId(adminId)) : true;

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const base = template ? instances.find((i) => i.id === template)?.config : undefined;
      const inst = await api.createInstance({
        name: name.trim(),
        installDir: dir || null,
        color,
        config: defaultConfig({
          ...(base ? { modifiers: base.modifiers, keys: base.keys, saveInterval: base.saveInterval, backups: base.backups, backupShort: base.backupShort, backupLong: base.backupLong } : {}),
          name: listName,
          port,
          world: world.trim(),
          password,
          public: listed,
          crossplay,
          preset,
        }),
        installNow,
        templateId: template || null,
      });
      useApp.getState().upsertInstance(inst);
      if (adminId.trim()) await api.setAccess(inst.id, 'admins', adminId.trim(), true).catch(() => {});
      setDialog(null);
      useApp.getState().openServer(inst.id, 'overview');
      useApp.getState().toast('success', installNow ? `${inst.name} created – downloading server files…` : `${inst.name} created`, inst.id);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={() => setDialog(null)}
      title="Create a Valheim server"
      description="Four quick steps. Everything can be changed later."
      icon={<Rocket />}
      width={680}
      footer={
        <>
          {step > 0 && (
            <Button variant="ghost" icon={<ArrowLeft className="size-3.5" />} onClick={() => setStep(step - 1)}>
              Back
            </Button>
          )}
          <div className="flex-1" />
          {step < STEPS.length - 1 ? (
            <Button variant="primary" iconRight={<ArrowRight className="size-3.5" />} disabled={!canNext} onClick={() => setStep(step + 1)}>
              Continue
            </Button>
          ) : (
            <Button variant="primary" icon={<Check className="size-4" />} loading={busy} onClick={create}>
              {installNow ? 'Create & install' : 'Create server'}
            </Button>
          )}
        </>
      }
    >
      <div className="mb-5 flex items-center gap-2">
        {STEPS.map((s, i) => (
          <div key={s.title} className="flex flex-1 items-center gap-2">
            <button
              disabled={i > step}
              onClick={() => setStep(i)}
              className={cn('flex items-center gap-2 rounded-lg px-2 py-1 text-[12px] font-medium whitespace-nowrap transition-colors', i === step ? 'text-fg' : i < step ? 'text-ok' : 'text-fg-4')}
            >
              <span className={cn('flex size-6 items-center justify-center rounded-full border text-[11px]', i === step ? 'border-accent bg-accent/15 text-accent' : i < step ? 'border-ok/40 bg-ok/10' : 'border-line-strong')}>
                {i < step ? <Check className="size-3" /> : i + 1}
              </span>
              {s.title}
            </button>
            {i < STEPS.length - 1 && <div className={cn('h-px flex-1', i < step ? 'bg-ok/40' : 'bg-line')} />}
          </div>
        ))}
      </div>

      <AnimatePresence mode="wait">
        <motion.div key={step} initial={{ opacity: 0, x: 8 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -8 }} transition={{ duration: 0.13 }} className="space-y-4">
          {step === 0 && (
            <>
              <div>
                <Label>Profile name</Label>
                <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="My Valheim Server" />
              </div>
              <div>
                <Label>Colour</Label>
                <div className="flex gap-2">
                  {Object.entries(SERVER_COLORS).map(([k, c]) => (
                    <button key={k} title={c.label} onClick={() => setColor(k)} className={cn('size-7 rounded-full border-2 transition-transform hover:scale-110', color === k ? 'border-fg' : 'border-transparent')} style={{ background: c.hex }} />
                  ))}
                </div>
              </div>
              <div>
                <Label>Install folder</Label>
                <div className="flex gap-2">
                  <Input mono value={dir} onChange={(e) => setDir(e.target.value)} placeholder="Automatic (inside the manager’s data folder)" />
                  <Button
                    icon={<FolderOpen className="size-3.5" />}
                    onClick={async () => {
                      const p = await pickFolder('Choose where to install the server');
                      if (p) setDir(p);
                    }}
                  >
                    Browse
                  </Button>
                </div>
                {probe && (
                  <div className={cn('mt-1.5 text-[11.5px]', !probe.writable ? 'text-err' : probe.hasServer ? 'text-ok' : 'text-fg-4')}>
                    {!probe.writable
                      ? 'This folder is not writable.'
                      : probe.hasServer
                        ? 'A Valheim server is already installed here – it is reused (use Add existing server to keep its start-script settings).'
                        : `${probe.empty ? 'Empty folder' : 'Folder has files'}${probe.freeBytes != null ? ` · ${bytes(probe.freeBytes)} free (needs ~3 GB)` : ''}`}
                  </div>
                )}
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <Label>Port (UDP)</Label>
                  <NumberInput value={port} min={1024} max={65534} onChange={setPort} />
                </div>
                <div className="flex items-end pb-2 text-[11.5px] text-fg-4">
                  Uses {port} and {port + 1}. A free pair is picked for you (2456/2457, then 2459/2460 …).
                </div>
              </div>
              {portClash && <div className="-mt-2 text-[11.5px] text-err">Ports overlap with {portClash.name}.</div>}
              {instances.length > 0 && (
                <div>
                  <Label>Copy modifiers, backups & automation from</Label>
                  <Select value={template} onChange={(e) => setTemplate(e.target.value)}>
                    <option value="">Defaults</option>
                    {instances.map((i) => (
                      <option key={i.id} value={i.id}>
                        {i.name}
                      </option>
                    ))}
                  </Select>
                </div>
              )}
            </>
          )}
          {step === 1 && (
            <>
              <div>
                <Label>Name in the server list</Label>
                <Input value={serverName} placeholder={name} onChange={(e) => setServerName(e.target.value)} maxLength={64} invalid={listName.includes('"')} />
                <div className="mt-1.5 text-[11.5px] text-fg-4">Players search for this under Join Game → Community.</div>
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <Label>Join password</Label>
                  <SecretInput value={password} onChange={setPassword} placeholder="Open server" generate invalid={!!pwProblem} />
                  <div className={cn('mt-1.5 text-[11.5px]', pwProblem ? 'text-err' : 'text-fg-4')}>{pwProblem ?? 'At least 5 characters, not part of the server name.'}</div>
                </div>
                <div>
                  <Label>World name</Label>
                  <Input value={world} onChange={(e) => setWorld(e.target.value)} maxLength={64} invalid={badWorld(world)} />
                  <div className="mt-1.5 text-[11.5px] text-fg-4">Generated with a random seed on the first start – or bring your own world under Worlds before starting.</div>
                </div>
              </div>
              <label className="flex items-center justify-between rounded-lg border border-line px-3 py-2 text-[12.5px] text-fg-2">
                List publicly (players find it in the Community server list)
                <Toggle size="sm" checked={listed} onChange={setListed} />
              </label>
              <label className="flex items-center justify-between gap-4 rounded-lg border border-line px-3 py-2 text-[12.5px] text-fg-2">
                <span>
                  Crossplay – Xbox, Game Pass and Microsoft Store players can join too, with a join code, and no port forwarding is needed
                </span>
                <Toggle size="sm" checked={crossplay} onChange={setCrossplay} />
              </label>
              <div>
                <Label>Your player id (optional)</Label>
                <Input mono value={adminId} onChange={(e) => setAdminId(e.target.value.trim())} placeholder="SteamID64 (76561198…) or Platform_Id (Xbox_…)" invalid={!!adminId && !isPlayerId(adminId)} />
                <div className="mt-1.5 text-[11.5px] text-fg-4">Added to adminlist.txt so you can kick, ban and save from the in-game console (F5). Your SteamID64 is in your Steam profile URL; in-game, F2 shows your id.</div>
              </div>
            </>
          )}
          {step === 2 && (
            <>
              <div className="grid grid-cols-2 gap-2">
                <button
                  onClick={() => setPreset('')}
                  className={cn('rounded-xl border p-3 text-left transition-colors', preset === '' ? 'border-accent/60 bg-accent/[0.07]' : 'border-line hover:border-line-strong')}
                >
                  <div className="text-[12.5px] font-semibold text-fg">Normal (no modifiers)</div>
                  <div className="text-[11px] text-fg-4">The game as designed. A world you bring keeps its own modifiers.</div>
                </button>
                {PRESETS.filter((p) => p.id !== 'normal').map((p) => (
                  <button
                    key={p.id}
                    onClick={() => setPreset(p.id)}
                    className={cn('rounded-xl border p-3 text-left transition-colors', preset === p.id ? 'border-accent/60 bg-accent/[0.07]' : 'border-line hover:border-line-strong')}
                  >
                    <div className="text-[12.5px] font-semibold text-fg">{p.label}</div>
                    <div className="text-[11px] text-fg-4">{p.blurb}</div>
                  </button>
                ))}
              </div>
              <div className="text-[11.5px] text-fg-4">Combat, death penalty, resources, raids, portals and the no-map / no-build-cost rules are under World Modifiers afterwards.</div>
            </>
          )}
          {step === 3 && (
            <>
              <div className="rounded-xl border border-line bg-bg-raised p-4">
                <dl className="grid grid-cols-[140px_1fr] gap-y-2 text-[12.5px]">
                  <dt className="text-fg-3">Profile</dt>
                  <dd className="text-fg">{name}</dd>
                  <dt className="text-fg-3">Server name</dt>
                  <dd className="text-fg">{listName}</dd>
                  <dt className="text-fg-3">Ports</dt>
                  <dd className="font-mono text-fg">
                    UDP {port}/{port + 1}
                    {crossplay && ' · crossplay'}
                  </dd>
                  <dt className="text-fg-3">World</dt>
                  <dd className="text-fg">
                    {world} · {preset ? presetLabel(preset) : 'Normal'}
                    {password ? ' · password' : ' · open'}
                    {listed ? ' · listed' : ' · unlisted'}
                  </dd>
                  <dt className="text-fg-3">Admin</dt>
                  <dd className="font-mono text-fg">{adminId || 'none yet'}</dd>
                  <dt className="text-fg-3">Folder</dt>
                  <dd className="truncate font-mono text-[11.5px] text-fg">{dir || 'automatic'}</dd>
                </dl>
              </div>
              <Checkbox checked={installNow} onChange={setInstallNow} label="Download the server files now with SteamCMD (about 1 GB download, 2 GB on disk, no Steam login needed)" />
              <Callout tone="info">
                After the download, press <b className="text-fg">Start</b> – a fresh world is ready within a minute. To host a world you played in single player, bring it over under <b className="text-fg">Worlds</b> before starting. Want mods? See <b className="text-fg">Mods</b>.
              </Callout>
              {error && <Callout tone="err">{error}</Callout>}
            </>
          )}
        </motion.div>
      </AnimatePresence>
    </Dialog>
  );
}
