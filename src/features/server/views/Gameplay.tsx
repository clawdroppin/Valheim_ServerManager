import { useCallback, useEffect, useState } from 'react';
import { Bookmark, Check, Info, RotateCcw, Save, Sparkles, SquareCheck, Swords, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { api, errMsg } from '@/lib/ipc';
import { KEYS, MODIFIERS, PRESETS, describeWorldKeys, presetLabel } from '@/data/gameplay';
import type { RulePreset, World } from '@/lib/types';
import { useApp } from '@/store/app';
import { confirm } from '@/components/ui/Dialog';
import { Button, IconButton } from '@/components/ui/Button';
import { Checkbox, FieldRow, Select } from '@/components/ui/Field';
import { Badge, Callout, PageHeader, Section } from '@/components/ui/Surface';
import { useDraft } from '../draft';

/** World modifiers: `-preset`, `-modifier` and `-setkey` on the command line. */
export function Gameplay({ id }: { id: string }) {
  const { draft, patch } = useDraft(id);
  const toast = useApp((s) => s.toast);
  const [world, setWorld] = useState<World | null>(null);
  const [mine, setMine] = useState<RulePreset[]>([]);
  const loadMine = useCallback(() => api.listRulePresets().then(setMine).catch(() => {}), []);

  useEffect(() => {
    loadMine();
    api.listWorlds(id).then((w) => setWorld(w.find((x) => x.active) ?? null)).catch(() => {});
  }, [id, loadMine]);
  if (!draft) return null;
  const c = draft.config;
  const anySet = !!c.preset || Object.values(c.modifiers).some((v) => v && v !== 'default') || c.keys.length > 0;
  const stored = world?.meta ? describeWorldKeys(world.meta.keys) : null;

  const setPreset = (p: string) =>
    patch((d) => {
      d.config.preset = d.config.preset === p ? '' : p;
    });
  const setModifier = (k: string, v: string) =>
    patch((d) => {
      const m = { ...d.config.modifiers };
      if (v) m[k] = v;
      else delete m[k];
      d.config.modifiers = m;
    });
  const toggleKey = (k: string, on: boolean) =>
    patch((d) => {
      d.config.keys = on ? [...d.config.keys.filter((x) => x !== k), k] : d.config.keys.filter((x) => x !== k);
    });

  const saveMine = async () => {
    const r = await confirm({
      title: 'Save these modifiers as a preset',
      message: 'Stores this server’s preset, modifiers and checkboxes under a name – apply them to any of your servers later. An existing preset with the same name is replaced.',
      input: '',
      inputPlaceholder: 'Preset name – e.g. Cozy building season',
      confirm: 'Save preset',
    });
    if (!r.ok) return;
    try {
      await api.saveRulePreset(r.value, { preset: c.preset, modifiers: c.modifiers, keys: c.keys });
      toast('success', `Saved preset “${r.value}”`, id);
      loadMine();
    } catch (e) {
      toast('error', errMsg(e), id);
    }
  };
  const applyMine = (p: RulePreset) => {
    patch((d) => {
      d.config.preset = p.settings.preset ?? '';
      d.config.modifiers = { ...(p.settings.modifiers ?? {}) };
      d.config.keys = [...(p.settings.keys ?? [])];
    });
    toast('success', `Applied “${p.name}” – review and save`, id);
  };
  const deleteMine = async (p: RulePreset) => {
    const r = await confirm({ title: `Delete preset “${p.name}”?`, message: 'Servers that use these modifiers keep them – only the saved preset is removed.', confirm: 'Delete', danger: true });
    if (!r.ok) return;
    try {
      await api.deleteRulePreset(p.name);
      loadMine();
    } catch (e) {
      toast('error', errMsg(e), id);
    }
  };

  return (
    <div className="mx-auto max-w-[1000px] space-y-5 p-6 pb-24">
      <PageHeader
        title="World Modifiers"
        description="Valheim’s difficulty and play-style settings. The server writes them into the world when it starts – the same options as the in-game world modifier screen."
        actions={
          <>
            <Button size="sm" icon={<Bookmark className="size-3.5" />} onClick={saveMine} disabled={!anySet}>
              Save as preset
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon={<RotateCcw className="size-3.5" />}
              disabled={!anySet}
              onClick={() =>
                patch((d) => {
                  d.config.preset = '';
                  d.config.modifiers = {};
                  d.config.keys = [];
                })
              }
            >
              Keep the world’s own
            </Button>
          </>
        }
      />

      <Callout tone="info" title={anySet ? 'These modifiers are applied at every start' : 'The world keeps its own modifiers'}>
        {anySet
          ? 'They replace whatever the world had – leave everything unset to keep the modifiers a world was created with (e.g. one you brought from single player).'
          : 'Nothing is passed to the server, so the world uses the modifiers stored in it. Pick a preset or a modifier to change them.'}
        {stored && (
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <span className="text-fg-3">“{world?.name}” has:</span>
            {stored.length ? stored.map((s) => <Badge key={s}>{s}</Badge>) : <Badge>Normal</Badge>}
          </div>
        )}
      </Callout>

      <Section title="Preset" description="-preset – a starting point. Modifiers below are applied on top of it." icon={<Sparkles />}>
        <div className="grid grid-cols-2 gap-2.5 md:grid-cols-4">
          {PRESETS.map((p) => (
            <button
              key={p.id}
              onClick={() => setPreset(p.id)}
              className={cn(
                'relative rounded-xl border p-3 text-left transition-colors',
                c.preset === p.id ? 'border-accent/60 bg-accent/10' : 'border-line bg-panel-2/50 hover:border-line-strong hover:bg-panel-2',
              )}
            >
              {c.preset === p.id && <Check className="absolute top-2.5 right-2.5 size-3.5 text-accent" />}
              <div className="text-[13px] font-semibold text-fg">{p.label}</div>
              <div className="mt-1 text-[11.5px] leading-snug text-fg-3">{p.blurb}</div>
            </button>
          ))}
        </div>
      </Section>

      <Section title="Modifiers" description="-modifier – each one overrides the preset." icon={<Swords />} bodyClassName="divide-y divide-line">
        {MODIFIERS.map((m) => (
          <FieldRow key={m.key} label={m.label} description={m.blurb} badge={c.modifiers[m.key] ? <Badge tone="accent">Changed</Badge> : undefined}>
            <Select value={c.modifiers[m.key] ?? ''} onChange={(e) => setModifier(m.key, e.target.value)} className="w-60">
              <option value="">{c.preset ? `As in ${presetLabel(c.preset)}` : 'Default'}</option>
              {m.options.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </Select>
          </FieldRow>
        ))}
      </Section>

      <Section title="Rules" description="-setkey – on/off world rules." icon={<SquareCheck />}>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {KEYS.map((k) => (
            <div key={k.key} className="rounded-xl border border-line bg-panel-2/40 p-3">
              <Checkbox checked={c.keys.includes(k.key)} onChange={(v) => toggleKey(k.key, v)} label={<span className="text-[13px] font-medium text-fg">{k.label}</span>} />
              <div className="mt-1 pl-6 text-[11.5px] text-fg-3">{k.blurb}</div>
            </div>
          ))}
        </div>
      </Section>

      <Section title="My presets" description="Your saved modifier sets – shared by all servers in this manager." icon={<Save />}>
        {mine.length === 0 ? (
          <div className="flex items-center gap-2 text-[12px] text-fg-4">
            <Info className="size-3.5" /> No saved presets yet – set some modifiers and use “Save as preset”.
          </div>
        ) : (
          <div className="divide-y divide-line">
            {mine.map((p) => {
              const chips = [
                p.settings.preset ? presetLabel(p.settings.preset) : null,
                ...Object.entries(p.settings.modifiers ?? {}).map(([k, v]) => `${MODIFIERS.find((m) => m.key === k)?.label ?? k}: ${MODIFIERS.find((m) => m.key === k)?.options.find((o) => o.value === v)?.label ?? v}`),
                ...(p.settings.keys ?? []).map((k) => KEYS.find((x) => x.key === k)?.label ?? k),
              ].filter(Boolean) as string[];
              return (
                <div key={p.name} className="flex items-center gap-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className="text-[13px] font-medium text-fg">{p.name}</div>
                    <div className="mt-1 flex flex-wrap gap-1">{chips.length ? chips.map((x) => <Badge key={x}>{x}</Badge>) : <Badge>World’s own</Badge>}</div>
                  </div>
                  <Button size="sm" onClick={() => applyMine(p)}>
                    Apply
                  </Button>
                  <IconButton label="Delete preset" onClick={() => deleteMine(p)}>
                    <Trash2 />
                  </IconButton>
                </div>
              );
            })}
          </div>
        )}
      </Section>
    </div>
  );
}
