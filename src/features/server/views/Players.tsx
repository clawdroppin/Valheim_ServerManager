import { useCallback, useEffect, useState } from 'react';
import { Ban, Clock, Copy, Crown, ExternalLink, History, LogIn, LogOut, RefreshCw, Search, Skull, UserX, Users } from 'lucide-react';
import { api, errMsg } from '@/lib/ipc';
import { ago, clock, dateTime, duration, mask } from '@/lib/format';
import { browse, copy } from '@/lib/shell';
import type { AccessView, OnlinePlayer, PlayerSummary, SessionRow } from '@/lib/types';
import { onlineCount, useApp, useStatus } from '@/store/app';
import { Button, IconButton } from '@/components/ui/Button';
import { confirm } from '@/components/ui/Dialog';
import { Input, Segmented } from '@/components/ui/Field';
import { Badge, Callout, Empty, PageHeader, Section } from '@/components/ui/Surface';

const steamProfile = (steamId: string) => `https://steamcommunity.com/profiles/${steamId}`;

function Avatar({ name }: { name: string }) {
  // A stable hue per name so the same player always looks the same.
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (
    <div className="flex size-7 shrink-0 items-center justify-center rounded-full text-[12px] font-semibold text-white/90" style={{ background: `oklch(0.42 0.09 ${h})` }}>
      {name.slice(0, 1).toUpperCase()}
    </div>
  );
}

function Since({ ts }: { ts: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  return <>{duration((now - ts) / 1000)}</>;
}

export function Players({ id }: { id: string }) {
  const st = useStatus(id);
  const events = useApp((s) => s.players[id]) ?? [];
  const toast = useApp((s) => s.toast);
  const streamer = useApp((s) => s.settings?.streamerMode ?? false);
  const [access, setAccess] = useState<AccessView | null>(null);
  const [known, setKnown] = useState<PlayerSummary[]>([]);
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [tab, setTab] = useState<'players' | 'sessions'>('players');
  const [q, setQ] = useState('');
  const running = st?.state === 'running' || st?.state === 'starting';
  const count = onlineCount(st);

  const load = useCallback(() => {
    api.getAccess(id).then(setAccess).catch((e) => toast('error', errMsg(e)));
    api.knownPlayers(id).then(setKnown).catch(() => {});
    api.playerSessions(id, 300).then(setSessions).catch(() => {});
  }, [id, toast]);
  useEffect(load, [load, events.length, st?.state]);

  const isAdmin = (sid: string | null, pid?: string | null) => (!!sid && !!access?.admins.some((a) => a.steamId === sid)) || (!!pid && !!access?.admins.some((a) => a.id === pid));
  const isBanned = (sid: string | null) => !!sid && !!access?.bans.some((a) => a.steamId === sid);
  /** What the lists need: the platform id (Xbox players have no SteamID), else the SteamID. */
  const listId = (p: OnlinePlayer) => p.platformId ?? p.steamId;
  const display = (p: { name: string; steamId: string | null }) => p.name || (p.steamId ? `Connecting… ${mask(p.steamId, streamer).slice(-6)}` : 'Connecting…');

  const setList = async (list: 'admins' | 'bans', sid: string, add: boolean, who: string) => {
    if (list === 'bans' && add) {
      const r = await confirm({ title: `Ban ${who}?`, message: 'Adds their id to bannedlist.txt – they can’t connect again. To remove them right now, an admin types “kick <name>” in the in-game console (F5).', confirm: 'Ban', danger: true });
      if (!r.ok) return;
    }
    try {
      await api.setAccess(id, list, sid, add);
      toast('success', `${who}: ${list === 'admins' ? (add ? 'now an admin' : 'admin removed') : add ? 'banned' : 'unbanned'}`, id);
      load();
    } catch (err) {
      toast('error', errMsg(err), id);
    }
  };
  const copyKick = (p: OnlinePlayer) => {
    copy(`kick ${p.name}`);
    toast('info', `Copied “kick ${p.name}” – paste it into the in-game console (F5) as an admin`, id);
  };
  const needle = q.trim().toLowerCase();
  const filteredKnown = known.filter((p) => !needle || p.name.toLowerCase().includes(needle));
  const filteredSessions = sessions.filter((s) => !needle || s.name.toLowerCase().includes(needle));
  const unnamed = running && st && st.queryPlayers != null && st.queryPlayers > st.players.length;

  return (
    <div className="mx-auto max-w-[1100px] space-y-5 p-6">
      <PageHeader
        title="Players"
        description="Who's online, everyone who has played here, and quick moderation."
        actions={
          <Button size="sm" icon={<RefreshCw className="size-3.5" />} onClick={load}>
            Refresh
          </Button>
        }
      />
      <div className="grid grid-cols-[1fr_380px] gap-4">
        <Section
          title={
            <span className="flex items-center gap-2">
              Online now <Badge tone={count ? 'ok' : 'neutral'}>{running ? `${count} / 10` : 'offline'}</Badge>
            </span>
          }
          description="Character names and ids from the server log; the count comes from the server itself (Steam query on port + 1). Deaths are counted this session."
          icon={<Users />}
          bodyClassName="py-3"
        >
          {!running ? (
            <Empty title="Server offline" description="Start the server to see who's connected." className="py-8" />
          ) : st?.players.length || unnamed ? (
            <div className="space-y-2">
              {st!.players.map((p) => (
                <div key={p.steamId ?? p.name} className="flex items-center gap-2.5 rounded-lg border border-line bg-bg-raised px-3 py-2">
                  <Avatar name={display(p)} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5 truncate text-[12.5px] font-medium text-fg">
                      {display(p)}
                      {isAdmin(p.steamId, p.platformId) && <Crown className="size-3.5 text-gold" />}
                    </div>
                    <div className="selectable truncate font-mono text-[10.5px] text-fg-4">
                      <Clock className="mr-1 inline size-2.5" />
                      <Since ts={p.since} />
                      {p.steamId ? ` · ${mask(p.steamId, streamer)}` : p.platformId ? ` · ${mask(p.platformId, streamer)}` : ''}
                      {p.deaths > 0 && ` · ${p.deaths} death${p.deaths === 1 ? '' : 's'}`}
                    </div>
                  </div>
                  {p.steamId && (
                    <IconButton label="Copy SteamID" onClick={() => copy(p.steamId!)}>
                      <Copy className="size-3.5" />
                    </IconButton>
                  )}
                  {p.steamId && (
                    <IconButton label="Open Steam profile" onClick={() => browse(steamProfile(p.steamId!))}>
                      <ExternalLink className="size-3.5" />
                    </IconButton>
                  )}
                  {listId(p) && (
                    <Button size="sm" variant="ghost" icon={<Crown className="size-3.5" />} onClick={() => setList('admins', listId(p)!, !isAdmin(p.steamId, p.platformId), display(p))}>
                      {isAdmin(p.steamId, p.platformId) ? 'Unadmin' : 'Admin'}
                    </Button>
                  )}
                  <Button size="sm" variant="ghost" icon={<UserX className="size-3.5" />} disabled={!p.name} title="Copies the kick command for the in-game console (Valheim has no remote console)" onClick={() => copyKick(p)}>
                    Kick…
                  </Button>
                  {listId(p) && (
                    <Button size="sm" variant="ghost" className="text-err" icon={<Ban className="size-3.5" />} onClick={() => setList('bans', listId(p)!, true, display(p))}>
                      Ban
                    </Button>
                  )}
                </div>
              ))}
              {unnamed && (
                <div className="rounded-lg border border-dashed border-line-strong px-3 py-2 text-[12px] text-fg-3">
                  +{st!.queryPlayers! - st!.players.length} more online – they joined before the manager was watching the log.
                </div>
              )}
            </div>
          ) : (
            <Empty title="Nobody online" description="Players show up here a few seconds after they join." className="py-8" />
          )}
        </Section>
        <Section title="This session" icon={<LogIn />} bodyClassName="py-2 max-h-[260px] overflow-y-auto">
          {events.length === 0 ? (
            <div className="py-6 text-center text-[12px] text-fg-4">No joins, leaves or deaths since the manager opened.</div>
          ) : (
            [...events].reverse().map((e, i) => (
              <div key={i} className="flex items-center gap-2.5 py-1.5 text-[12.5px]">
                {e.kind === 'join' ? <LogIn className="size-3.5 text-ok" /> : e.kind === 'death' ? <Skull className="size-3.5 text-err" /> : <LogOut className="size-3.5 text-fg-4" />}
                <span className="text-fg">{e.name}</span>
                <span className="text-fg-3">{e.kind === 'join' ? 'joined' : e.kind === 'death' ? 'died' : 'left'}</span>
                <span className="ml-auto font-mono text-[11px] text-fg-4">{clock(e.ts)}</span>
              </div>
            ))
          )}
        </Section>
      </div>

      <Section
        title="History"
        description="Every player the manager has seen on this server, kept in its own database."
        icon={<History />}
        actions={
          <>
            <div className="relative w-56">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-fg-4" />
              <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter by name" className="h-7 pl-8" />
            </div>
            <Segmented
              size="sm"
              value={tab}
              onChange={setTab}
              options={[
                { value: 'players', label: `Players (${known.length})` },
                { value: 'sessions', label: 'Sessions' },
              ]}
            />
          </>
        }
        bodyClassName="py-2"
      >
        {tab === 'players' ? (
          filteredKnown.length === 0 ? (
            <div className="py-6 text-center text-[12px] text-fg-4">Nobody yet – players are recorded as they join.</div>
          ) : (
            <table className="w-full text-[12.5px]">
              <thead>
                <tr className="text-left text-[10.5px] tracking-wide text-fg-4 uppercase">
                  <th className="py-2 font-medium">Player</th>
                  <th className="font-medium">SteamID</th>
                  <th className="font-medium">Sessions</th>
                  <th className="font-medium">Play time</th>
                  <th className="font-medium">First seen</th>
                  <th className="text-right font-medium">Last seen</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {filteredKnown.map((p) => {
                  const on = st?.players.some((x) => x.name === p.name || (!!p.steamId && x.steamId === p.steamId));
                  return (
                    <tr key={p.name}>
                      <td className="py-2">
                        <span className="flex items-center gap-2">
                          <Avatar name={p.name} />
                          <span className="font-medium text-fg">{p.name}</span>
                          {on && <Badge tone="ok">online</Badge>}
                          {isAdmin(p.steamId) && <Crown className="size-3.5 text-gold" />}
                          {isBanned(p.steamId) && <Badge tone="err">banned</Badge>}
                        </span>
                      </td>
                      <td className="selectable font-mono text-[11.5px] text-fg-3">
                        {p.steamId ? (
                          <span className="flex items-center gap-1">
                            {mask(p.steamId, streamer)}
                            <IconButton label="Copy SteamID" onClick={() => copy(p.steamId!)}>
                              <Copy className="size-3" />
                            </IconButton>
                            <IconButton label="Open Steam profile" onClick={() => browse(steamProfile(p.steamId!))}>
                              <ExternalLink className="size-3" />
                            </IconButton>
                          </span>
                        ) : (
                          '—'
                        )}
                      </td>
                      <td className="font-mono text-fg-2">{p.sessions}</td>
                      <td className="font-mono text-fg-2">{duration(p.totalMs / 1000)}</td>
                      <td className="text-fg-3">{dateTime(p.firstSeen)}</td>
                      <td className="text-right text-fg-3">{on ? 'now' : ago(p.lastSeen)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )
        ) : filteredSessions.length === 0 ? (
          <div className="py-6 text-center text-[12px] text-fg-4">No sessions recorded yet.</div>
        ) : (
          <div className="max-h-[420px] divide-y divide-line overflow-y-auto">
            {filteredSessions.map((s) => (
              <div key={s.id} className="flex items-center gap-3 py-2 text-[12.5px]">
                <Avatar name={s.name} />
                <span className="font-medium text-fg">{s.name}</span>
                <span className="text-fg-3">{dateTime(s.joinedAt)}</span>
                <span className="ml-auto font-mono text-[11.5px] text-fg-2">{s.leftAt ? duration((s.leftAt - s.joinedAt) / 1000) : <Badge tone="ok">playing</Badge>}</span>
              </div>
            ))}
          </div>
        )}
      </Section>

      <Callout tone="info" title="How players are tracked">
        A connection shows up in the server log with the player’s SteamID (“Got connection SteamID …”); it counts as a join once their character spawns (“Got character ZDOID from …”), which
        also reports deaths. Connections that never spawn a character – wrong password, cancelled – aren’t sessions. Valheim has no remote console, so kicking is done in-game by an admin; bans
        go into bannedlist.txt.
      </Callout>
    </div>
  );
}
