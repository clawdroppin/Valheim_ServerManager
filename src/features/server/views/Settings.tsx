import { Gamepad2, Globe2, HardDriveDownload, IdCard, KeyRound, Network, Palette, Wand2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { api } from '@/lib/ipc';
import { SERVER_COLORS } from '@/lib/colors';
import { secondsLabel } from '@/data/gameplay';
import type { ServerInstance } from '@/lib/types';
import { useApp } from '@/store/app';
import { Button } from '@/components/ui/Button';
import { FieldRow, Input, NumberInput, SecretInput, Slider, TextArea, Toggle } from '@/components/ui/Field';
import { Badge, Callout, PageHeader, Section } from '@/components/ui/Surface';
import { useDraft } from '../draft';

const usedPorts = (i: ServerInstance) => [i.config.port, i.config.port + 1];

/** The server's own password rules (verified: a short one makes a public server hang half-started). */
export function passwordProblem(name: string, pw: string, isPublic = true): string | undefined {
  if (!pw) return undefined;
  if (pw.length < 5 && isPublic) return 'At least 5 characters – a public server refuses shorter passwords.';
  if (name.trim() && name.toLowerCase().includes(pw.toLowerCase())) return 'The password can’t be part of the server name.';
  if (/[\s"]/.test(pw)) return 'No spaces or double quotes.';
  return undefined;
}

export function Settings({ id }: { id: string }) {
  const { draft, patch } = useDraft(id);
  const instances = useApp((s) => s.instances);
  if (!draft) return null;
  const c = draft.config;
  const mine = usedPorts(draft);
  const clash = instances.filter((o) => o.id !== id && usedPorts(o).some((p) => mine.includes(p)));
  const badWorld = !c.world.trim() || /[\\/:*?"<>|]/.test(c.world) || c.world.startsWith('.') || c.world.includes('_backup_');
  const pwProblem = passwordProblem(c.name, c.password, c.public);

  return (
    <div className="mx-auto max-w-[1000px] space-y-5 p-6 pb-24">
      <PageHeader
        title="Server Settings"
        description="Valheim takes every setting on its command line – the manager builds it from these values when the server starts, so changes made while it runs apply at the next restart."
      />

      <Section title="Listing" description="What players see when they look for the server." icon={<Globe2 />} bodyClassName="divide-y divide-line">
        <FieldRow label="Server name" description="-name – shown in the community server list; this is what players search for." badge={<Badge tone="err">Required</Badge>} error={!c.name.trim() ? 'The server needs a name.' : c.name.includes('"') ? 'No double quotes.' : undefined}>
          <Input value={c.name} invalid={!c.name.trim() || c.name.includes('"')} maxLength={64} onChange={(e) => patch((d) => void (d.config.name = e.target.value))} />
        </FieldRow>
        <FieldRow label="List publicly" description="-public 1 – players find the server under Join Game → Community. Off = only Join IP (and the join code with Crossplay).">
          <Toggle checked={c.public} onChange={(v) => patch((d) => void (d.config.public = v))} />
        </FieldRow>
        <FieldRow
          label="Crossplay"
          description="-crossplay – runs on the PlayFab backend so Xbox / Microsoft Store / Game Pass players can join too. Players connect through a relay (no port forwarding needed) with a 6-digit join code, your public IP, or the server list."
        >
          <Toggle checked={c.crossplay} onChange={(v) => patch((d) => void (d.config.crossplay = v))} />
        </FieldRow>
        {c.crossplay && (
          <FieldRow label="Instance id" description="-instanceid – only needed when several crossplay servers share the same port on one PC (gives each its own PlayFab id).">
            <Input value={c.instanceId} mono maxLength={32} placeholder="Not set" onChange={(e) => patch((d) => void (d.config.instanceId = e.target.value.replace(/[^\w-]/g, '')))} className="w-60" />
          </FieldRow>
        )}
        <FieldRow label="Players" description="Valheim allows 10 players on a vanilla server (mods can raise it).">
          <span className="font-mono text-[13px] text-fg-2">10</span>
        </FieldRow>
      </Section>

      <Section title="Access" description="Who can join. Admins, bans and the allow list are under Access & Bans." icon={<KeyRound />} bodyClassName="divide-y divide-line">
        <FieldRow label="Join password" description="-password – at least 5 characters and not part of the server name. Empty = anyone who finds the server can join." error={pwProblem}>
          <SecretInput value={c.password} onChange={(v) => patch((d) => void (d.config.password = v))} placeholder="No password" generate />
        </FieldRow>
        {!c.password && c.public && <Callout tone="warn">The server is listed without a password – anyone can join. Set one, or keep an allow list under Access & Bans.</Callout>}
      </Section>

      <Section
        title="Network"
        description="Valheim uses the port you pick and the next one (UDP). Forward both for Steam players outside your network – see Port Forwarding."
        icon={<Network />}
        actions={
          <Button
            size="sm"
            icon={<Wand2 className="size-3.5" />}
            onClick={async () => {
              const p = await api.suggestPorts(id);
              patch((d) => void (d.config.port = p.port));
            }}
          >
            Pick a free port
          </Button>
        }
        bodyClassName="divide-y divide-line"
      >
        <FieldRow
          label="Port"
          description={`-port – UDP ${c.port} (game) and ${c.port + 1} (Steam query). Default 2456; a second server on this PC gets 2459, and so on.`}
          error={clash.length ? `Ports overlap with ${clash.map((x) => x.name).join(', ')} – they can't run at the same time.` : undefined}
        >
          <NumberInput value={c.port} min={1024} max={65534} onChange={(v) => patch((d) => void (d.config.port = v))} className="w-44" />
        </FieldRow>
      </Section>

      <Section title="World & saves" description="Which world the server loads and how it protects it. Switch, import and roll back worlds under Worlds." icon={<HardDriveDownload />} bodyClassName="divide-y divide-line">
        <FieldRow label="World" description="-world – a folder under worlds_local. A new name generates a fresh world (with a random seed) on the next start." error={badWorld ? 'Use a plain folder name.' : undefined}>
          <Input value={c.world} invalid={badWorld} maxLength={64} onChange={(e) => patch((d) => void (d.config.world = e.target.value))} />
        </FieldRow>
        <FieldRow label="Save every" description="-saveinterval – the world is also saved on every clean stop. Default 30 minutes.">
          <div className="flex items-center gap-3">
            <Slider value={c.saveInterval} min={300} max={3600} step={60} marks={[300, 900, 1800, 3600]} onChange={(v) => patch((d) => void (d.config.saveInterval = v))} />
            <span className="w-16 text-right font-mono text-[12.5px] text-fg">{secondsLabel(c.saveInterval)}</span>
          </div>
        </FieldRow>
        <FieldRow label="Server backups" description="-backups – world backups the game keeps next to the world (roll back to them under Worlds). The manager's zip backups come on top.">
          <NumberInput value={c.backups} min={0} max={50} onChange={(v) => patch((d) => void (d.config.backups = v))} className="w-40" />
        </FieldRow>
        <FieldRow label="First backup after" description="-backupshort – age of the first (short) backup. Default 2 h.">
          <div className="flex items-center gap-3">
            <Slider value={c.backupShort} min={600} max={21600} step={600} onChange={(v) => patch((d) => void (d.config.backupShort = v))} />
            <span className="w-16 text-right font-mono text-[12.5px] text-fg">{secondsLabel(c.backupShort)}</span>
          </div>
        </FieldRow>
        <FieldRow label="Then every" description="-backuplong – interval between the later backups. Default 12 h.">
          <div className="flex items-center gap-3">
            <Slider value={c.backupLong} min={3600} max={172800} step={3600} onChange={(v) => patch((d) => void (d.config.backupLong = v))} />
            <span className="w-16 text-right font-mono text-[12.5px] text-fg">{secondsLabel(c.backupLong)}</span>
          </div>
        </FieldRow>
      </Section>

      <Section title="Manager profile" description="Only used by this app – not sent to the game." icon={<IdCard />} bodyClassName="divide-y divide-line">
        <FieldRow label="Display name" description="Name of this profile in tabs and the sidebar.">
          <Input value={draft.name} onChange={(e) => patch((d) => void (d.name = e.target.value))} />
        </FieldRow>
        <FieldRow
          label={
            <span className="flex items-center gap-2">
              <Palette className="size-3.5 text-fg-3" /> Tag colour
            </span>
          }
          description="Helps tell servers apart at a glance."
        >
          <div className="flex flex-wrap gap-2">
            {Object.entries(SERVER_COLORS).map(([k, col]) => (
              <button
                key={k}
                title={col.label}
                onClick={() => patch((d) => void (d.color = k))}
                className={cn('size-7 rounded-full border-2 transition-transform hover:scale-110', draft.color === k ? 'border-fg' : 'border-transparent')}
                style={{ background: col.hex }}
              />
            ))}
          </div>
        </FieldRow>
        <FieldRow
          label={
            <span className="flex items-center gap-2">
              <Gamepad2 className="size-3.5 text-fg-3" /> Notes
            </span>
          }
          description="Anything you want to remember about this server."
          stacked
        >
          <TextArea value={draft.notes} onChange={(e) => patch((d) => void (d.notes = e.target.value))} placeholder="Discord invite, season dates, who the admins are…" />
        </FieldRow>
      </Section>
    </div>
  );
}
