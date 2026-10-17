import { useCallback, useEffect, useMemo, useState } from 'react';
import CodeMirror from '@uiw/react-codemirror';
import { StreamLanguage, HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { EditorView } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { AlertTriangle, FileCode2, FolderOpen, History, Puzzle, RotateCcw, Save } from 'lucide-react';
import { cn } from '@/lib/cn';
import { api, errMsg, listen } from '@/lib/ipc';
import { ago, dateTime } from '@/lib/format';
import { reveal } from '@/lib/shell';
import type { ConfigFile, ConfigFileKind } from '@/lib/types';
import { useApp, useStatus } from '@/store/app';
import { Button, IconButton } from '@/components/ui/Button';
import { Segmented, Select } from '@/components/ui/Field';
import { Callout } from '@/components/ui/Surface';

const theme = EditorView.theme(
  {
    '&': { backgroundColor: 'transparent', color: '#e3ecf2' },
    '.cm-content': { caretColor: '#f0a24a', padding: '12px 0' },
    '.cm-gutters': { backgroundColor: 'transparent', color: '#4f616c', border: 'none' },
    '.cm-activeLine': { backgroundColor: 'rgb(214 232 245 / 0.03)' },
    '.cm-activeLineGutter': { backgroundColor: 'transparent', color: '#b6c3cc' },
    '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'rgb(217 130 43 / 0.28) !important' },
    '.cm-cursor': { borderLeftColor: '#f0a24a' },
    '.cm-searchMatch': { backgroundColor: 'rgb(250 204 21 / 0.25)' },
    '.cm-panels': { backgroundColor: '#161e25', color: '#e3ecf2' },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: [t.propertyName, t.definition(t.propertyName)], color: '#f6b765' },
  { tag: [t.string, t.atom], color: '#9fd8a8' },
  { tag: t.number, color: '#a99cf8' },
  { tag: [t.heading, t.keyword], color: '#7dd3fc' },
  { tag: t.comment, color: '#5f7380', fontStyle: 'italic' },
  { tag: t.punctuation, color: '#7d8f9b' },
]);

/** BepInEx .cfg files are INI-style: `[Section]`, `## comments`, `Key = Value`. */
const cfgExtensions = [StreamLanguage.define(properties), syntaxHighlighting(highlight), theme, EditorView.lineWrapping];
const textExtensions = [theme, EditorView.lineWrapping];

const LISTS: { id: ConfigFileKind; name: string; blurb: string }[] = [
  { id: 'admins', name: 'adminlist.txt', blurb: 'One player id per line (Steam_7656… / Xbox_…); lines starting with // are comments. Admins can use the in-game console.' },
  { id: 'bans', name: 'bannedlist.txt', blurb: 'One player id per line. Admins’ in-game bans are written here too.' },
  { id: 'permitted', name: 'permittedlist.txt', blurb: 'One player id per line. While anyone is on it, ONLY these players can join.' },
];

/** What's open: a permission list, or a BepInEx config file by name. */
type Source = { kind: 'list'; id: ConfigFileKind } | { kind: 'mod'; name: string };

function checkList(text: string): { line: number; message: string } | null {
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i].trim();
    if (l && !l.startsWith('//') && !/^7656\d{13}$/.test(l) && !/^(Steam|Xbox|PlayFab|None)_[A-Za-z0-9-]{3,64}$/.test(l)) return { line: i + 1, message: `“${l.slice(0, 24)}” isn't a player id` };
  }
  return null;
}

export function ConfigFiles({ id }: { id: string }) {
  const st = useStatus(id);
  const toast = useApp((s) => s.toast);
  const [loaded, setLoaded] = useState<ConfigFile | null>(null);
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const [history, setHistory] = useState<[string, number][]>([]);
  const [modFiles, setModFiles] = useState<string[]>([]);
  const [src, setSrc] = useState<Source>({ kind: 'list', id: 'admins' });
  const listMeta = src.kind === 'list' ? LISTS.find((f) => f.id === src.id)! : null;
  const title = listMeta ? listMeta.name : src.kind === 'mod' ? src.name : '';
  const dirty = loaded != null && text !== loaded.text;
  const running = st?.state === 'running' || st?.state === 'starting';

  const loadMods = useCallback(() => {
    api
      .modsStatus(id)
      .then((m) => setModFiles(m.configs.map((c) => c.name)))
      .catch(() => setModFiles([]));
  }, [id]);
  useEffect(() => {
    loadMods();
    const un = listen<string>('mods-changed', (sid) => sid === id && loadMods());
    return () => void un.then((f) => f());
  }, [id, loadMods]);

  const load = useCallback(async () => {
    try {
      const c = src.kind === 'list' ? await api.readConfig(id, src.id) : await api.readModConfig(id, src.name);
      setLoaded(c);
      setText(c.text);
      (src.kind === 'list' ? api.configHistory(id, src.id) : api.modConfigHistory(id, src.name)).then(setHistory).catch(() => setHistory([]));
    } catch (e) {
      toast('error', errMsg(e));
    }
  }, [id, src, toast]);
  useEffect(() => {
    load();
  }, [load, st?.state]);

  const problem = useMemo(() => (src.kind === 'list' && text.trim() ? checkList(text) : null), [text, src.kind]);

  const save = useCallback(async () => {
    setSaving(true);
    try {
      if (src.kind === 'list') {
        await api.writeConfig(id, src.id, text);
        toast('success', `${title} saved${running ? ' – the server re-reads it' : ''}`, id);
      } else {
        const wasRunning = await api.writeModConfig(id, src.name, text);
        toast('success', wasRunning ? `${title} saved – restart the server so the mod reads it` : `${title} saved`, id);
      }
      await load();
    } catch (e) {
      toast('error', errMsg(e), id);
    } finally {
      setSaving(false);
    }
  }, [id, src, title, text, load, toast, running]);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === 's' && dirty && !problem) {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [dirty, problem, save]);

  const switchTo = async (next: Source) => {
    if (dirty) {
      const { confirm } = await import('@/components/ui/Dialog');
      const r = await confirm({ title: 'Discard unsaved edits?', message: `Your changes to ${title} are lost.`, confirm: 'Discard', danger: true });
      if (!r.ok) return;
    }
    setSrc(next);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-3 border-b border-line px-4 py-2.5">
        {src.kind === 'mod' ? <Puzzle className="size-4 text-accent" /> : <FileCode2 className="size-4 text-accent" />}
        <div className="min-w-0 flex-1">
          <div className="truncate text-[13px] font-semibold text-fg">{title}</div>
          <div className="text-[11px] text-fg-4">{loaded?.modified ? `Modified ${ago(loaded.modified)}` : 'Not created yet'}</div>
        </div>
        <Segmented
          size="sm"
          value={src.kind === 'list' ? src.id : ('' as ConfigFileKind)}
          onChange={(v) => switchTo({ kind: 'list', id: v })}
          options={LISTS.map((f) => ({ value: f.id, label: f.name.replace(/list\.txt$/, '') }))}
        />
        {modFiles.length > 0 && (
          <Select value={src.kind === 'mod' ? src.name : ''} className="w-56" onChange={(e) => e.target.value && switchTo({ kind: 'mod', name: e.target.value })}>
            <option value="">Mod configs ({modFiles.length})…</option>
            {modFiles.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </Select>
        )}
        {history.length > 0 && (
          <Select
            value=""
            className="w-44"
            onChange={async (e) => {
              if (!e.target.value) return;
              try {
                setText(await api.readTextFile(id, e.target.value));
                toast('info', 'Loaded an earlier version – review it and Save to restore');
              } catch (err) {
                toast('error', errMsg(err));
              }
            }}
          >
            <option value="">History ({history.length})</option>
            {history.map(([p, ts]) => (
              <option key={p} value={p}>
                {dateTime(ts)} · {ago(ts)}
              </option>
            ))}
          </Select>
        )}
        <IconButton label="Open folder" onClick={() => loaded && reveal(loaded.path.replace(/[\\/][^\\/]+$/, ''))}>
          <FolderOpen />
        </IconButton>
        <Button size="sm" variant="ghost" icon={<RotateCcw className="size-3.5" />} disabled={!dirty} onClick={() => setText(loaded?.text ?? '')}>
          Revert
        </Button>
        <Button size="sm" variant="primary" icon={<Save className="size-3.5" />} disabled={!dirty || !!problem} loading={saving} onClick={save}>
          Save
        </Button>
      </div>
      <div className="space-y-2 px-4 pt-3">
        {running && src.kind === 'mod' && <Callout tone="info">The server is running. Mods read their config at start (some watch the file) – restart the server to be sure the change applies.</Callout>}
        <div className="text-[11.5px] leading-relaxed text-fg-4">
          {listMeta ? listMeta.blurb : 'A BepInEx mod configuration: [Sections], ## comments and Key = Value lines. The mod writes its defaults here on its first start.'} The previous 30 versions of each file are kept.
        </div>
        {loaded && !loaded.exists && <Callout tone="info">The file doesn’t exist yet – it is created when you save (the server also creates it on its first start).</Callout>}
      </div>
      <div className="m-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-bg/70">
        <div className="min-h-0 flex-1 overflow-auto">
          <CodeMirror value={text} onChange={setText} extensions={src.kind === 'mod' ? cfgExtensions : textExtensions} theme="none" basicSetup={{ foldGutter: true, highlightActiveLine: true }} height="100%" style={{ height: '100%' }} />
        </div>
        <div className="flex shrink-0 items-center gap-4 border-t border-line px-4 py-1.5 font-mono text-[10.5px] text-fg-4">
          <span className="truncate">{loaded?.path}</span>
          {problem ? (
            <span className={cn('ml-auto flex items-center gap-1.5 text-err')}>
              <AlertTriangle className="size-3" /> Line {problem.line}: {problem.message}
            </span>
          ) : (
            <span className="ml-auto text-ok">{src.kind === 'list' ? 'Valid list' : 'Mod config'}</span>
          )}
          {history.length > 0 && (
            <span className="flex items-center gap-1">
              <History className="size-3" /> {history.length} versions kept
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
