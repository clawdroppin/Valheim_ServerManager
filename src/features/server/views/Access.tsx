import { useCallback, useEffect, useState } from 'react';
import { Ban, Copy, Crown, ExternalLink, ListChecks, Plus, RefreshCw, ShieldCheck, Trash2, UserPlus } from 'lucide-react';
import { api, errMsg, listen } from '@/lib/ipc';
import { copy, browse } from '@/lib/shell';
import { mask } from '@/lib/format';
import type { AccessEntry, AccessList, AccessView } from '@/lib/types';
import { useApp, useStatus } from '@/store/app';
import { Button, IconButton } from '@/components/ui/Button';
import { confirm } from '@/components/ui/Dialog';
import { Input } from '@/components/ui/Field';
import { Badge, Callout, Empty, PageHeader, Section } from '@/components/ui/Surface';

/** A SteamID64, or `Platform_Id` as the lists use it (Steam_7656…, Xbox_…, PlayFab_…). */
export const isPlayerId = (s: string) => /^7656\d{13}$/.test(s.trim()) || /^(Steam|Xbox|PlayFab|None)_[A-Za-z0-9-]{3,64}$/.test(s.trim());

function Avatar({ seed }: { seed: string }) {
  let h = 0;
  for (const c of seed) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (
    <div className="flex size-7 shrink-0 items-center justify-center rounded-full text-[12px] font-semibold text-white/90" style={{ background: `oklch(0.42 0.09 ${h})` }}>
      {(seed[0] ?? '?').toUpperCase()}
    </div>
  );
}

function AddRow({ placeholder, label, onAdd }: { placeholder: string; label: string; onAdd: (pid: string) => Promise<void> }) {
  const [v, setV] = useState('');
  const [busy, setBusy] = useState(false);
  const ok = isPlayerId(v);
  const submit = async () => {
    if (!ok) return;
    setBusy(true);
    try {
      await onAdd(v.trim());
      setV('');
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-center gap-2">
      <Input mono value={v} onChange={(e) => setV(e.target.value.replace(/\s/g, ''))} onKeyDown={(e) => e.key === 'Enter' && submit()} placeholder={placeholder} invalid={!!v && !ok} className="flex-1" />
      <Button size="md" variant="primary" icon={<Plus className="size-3.5" />} disabled={!ok} loading={busy} onClick={submit}>
        {label}
      </Button>
    </div>
  );
}

function EntryRow({ e, streamer, onRemove, removeLabel }: { e: AccessEntry; streamer: boolean; onRemove: () => void; removeLabel: string }) {
  return (
    <div className="flex items-center gap-3 py-2.5">
      <Avatar seed={e.name ?? e.id.replace(/^\w+_/, '')} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-[12.5px] font-medium text-fg">
          {e.name ?? <span className="text-fg-3">Not seen on this server yet</span>}
          {e.online && <Badge tone="ok">online</Badge>}
          {!e.steamId && <Badge>{e.id.split('_')[0]}</Badge>}
        </div>
        <div className="selectable mt-0.5 font-mono text-[11px] text-fg-4">{mask(e.id, streamer)}</div>
      </div>
      <IconButton label="Copy id" onClick={() => copy(e.id)}>
        <Copy className="size-3.5" />
      </IconButton>
      {e.steamId && (
        <IconButton label="Steam profile" onClick={() => browse(`https://steamcommunity.com/profiles/${e.steamId}`)}>
          <ExternalLink className="size-3.5" />
        </IconButton>
      )}
      <Button size="sm" variant="outline" icon={<Trash2 className="size-3.5" />} onClick={onRemove}>
        {removeLabel}
      </Button>
    </div>
  );
}

const NOUN: Record<AccessList, [string, string]> = {
  admins: ['Admin added', 'Admin removed'],
  bans: ['Banned', 'Unbanned'],
  permitted: ['Added to the allow list', 'Removed from the allow list'],
};

export function Access({ id }: { id: string }) {
  const st = useStatus(id);
  const toast = useApp((s) => s.toast);
  const streamer = useApp((s) => s.settings?.streamerMode ?? false);
  const [view, setView] = useState<AccessView | null>(null);
  const running = st?.state === 'running' || st?.state === 'starting';

  const load = useCallback(() => {
    api.getAccess(id).then(setView).catch((e) => toast('error', errMsg(e), id));
  }, [id, toast]);
  useEffect(() => {
    load();
    const un = listen<string>('access-changed', (sid) => sid === id && load());
    return () => void un.then((f) => f());
  }, [id, load]);

  const edit = async (list: AccessList, pid: string, add: boolean, ask?: string) => {
    if (ask) {
      const r = await confirm({ title: ask, message: list === 'admins' ? 'They lose admin rights (the server re-reads the list when it changes).' : list === 'bans' ? 'They can join again.' : 'They can no longer join while the allow list is in use.', confirm: 'Remove' });
      if (!r.ok) return;
    }
    try {
      const changed = await api.setAccess(id, list, pid, add);
      if (changed) toast('success', `${NOUN[list][add ? 0 : 1]} – ${running ? 'the server picks it up from the file' : 'saved'}`, id);
      else toast('info', 'Nothing changed – already on the list', id);
      load();
    } catch (e) {
      toast('error', errMsg(e), id);
    }
  };

  const list = (key: AccessList, entries: AccessEntry[] | undefined, removeLabel: string, ask: (e: AccessEntry) => string) =>
    !entries ? (
      <div className="py-4 text-center text-[12px] text-fg-4">Loading…</div>
    ) : (
      <div className="divide-y divide-line">
        {entries.map((e) => (
          <EntryRow key={e.id} e={e} streamer={streamer} removeLabel={removeLabel} onRemove={() => edit(key, e.id, false, ask(e))} />
        ))}
      </div>
    );

  return (
    <div className="mx-auto max-w-[1000px] space-y-5 p-6 pb-16">
      <PageHeader
        title="Access & bans"
        description="adminlist.txt, bannedlist.txt and permittedlist.txt in the save folder – one Platform_UserId per line (Steam_7656…, Xbox_…). Admins edit the same files in-game with ban / unban."
        actions={
          <Button size="sm" icon={<RefreshCw className="size-3.5" />} onClick={load}>
            Refresh
          </Button>
        }
      />

      <Section
        title={
          <span className="flex items-center gap-2">
            Admins <Badge tone={view?.admins.length ? 'accent' : 'neutral'}>{view?.admins.length ?? 0}</Badge>
          </span>
        }
        description="Admins can use the in-game console (F5): kick, ban, unban, banned, save … and devcommands where the game allows it."
        icon={<Crown />}
        bodyClassName="space-y-3 py-3"
      >
        <AddRow placeholder="SteamID64 or Platform_Id – e.g. 76561198012345678 / Xbox_2535…" label="Make admin" onAdd={(p) => edit('admins', p, true)} />
        {view?.admins.length === 0 ? (
          <Empty icon={<ShieldCheck />} title="No admins yet" description="Add your own id so you can manage the server in-game." className="py-6" />
        ) : (
          list('admins', view?.admins, 'Remove', (e) => `Remove ${e.name ?? e.id} as admin?`)
        )}
      </Section>

      <Section
        title={
          <span className="flex items-center gap-2">
            Banned <Badge tone={view?.bans.length ? 'err' : 'neutral'}>{view?.bans.length ?? 0}</Badge>
          </span>
        }
        description="Banned players can't connect. In-game bans by admins land in this list too."
        icon={<Ban />}
        bodyClassName="space-y-3 py-3"
      >
        <AddRow placeholder="SteamID64 or Platform_Id to ban" label="Ban" onAdd={(p) => edit('bans', p, true)} />
        {view?.bans.length === 0 ? <div className="py-4 text-center text-[12px] text-fg-4">Nobody is banned.</div> : list('bans', view?.bans, 'Unban', (e) => `Unban ${e.name ?? e.id}?`)}
      </Section>

      <Section
        title={
          <span className="flex items-center gap-2">
            Allow list <Badge tone={view?.permitted.length ? 'warn' : 'neutral'}>{view?.permitted.length ?? 0}</Badge>
          </span>
        }
        description="permittedlist.txt – while it has anyone on it, ONLY these players can join; everyone else is kicked. Leave it empty for an open (or password-protected) server."
        icon={<ListChecks />}
        bodyClassName="space-y-3 py-3"
      >
        {!!view?.permitted.length && <Callout tone="warn">The allow list is active – make sure your own id is on it.</Callout>}
        <AddRow placeholder="SteamID64 or Platform_Id to allow" label="Allow" onAdd={(p) => edit('permitted', p, true)} />
        {view?.permitted.length === 0 ? (
          <div className="py-4 text-center text-[12px] text-fg-4">Empty – everyone who knows the password can join.</div>
        ) : (
          list('permitted', view?.permitted, 'Remove', (e) => `Remove ${e.name ?? e.id} from the allow list?`)
        )}
      </Section>

      <Callout tone="info" title="Finding a player’s id">
        <UserPlus className="mr-1 inline size-3.5" /> Players who joined this server are listed under Players with their id – use the buttons there. In the game, each player can open the F2 panel to see
        their own id. For Steam players the SteamID64 also works: the number in <span className="font-mono">steamcommunity.com/profiles/7656…</span>.
      </Callout>
    </div>
  );
}
