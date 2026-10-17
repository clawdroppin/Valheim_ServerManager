// Valheim world modifiers – the values the dedicated server accepts on its command line
// (`-preset`, `-modifier <key> <value>`, `-setkey <key>`), from the Iron Gate server manual shipped
// with build 25909431. The server writes them into the world's .fwl2 at start (verified: a world
// started with `-preset hard -modifier raids none -setkey nomap` stores
// `preset combat_hard:deathpenalty_default:resources_default:raids_none:portals_default` and `nomap`).

export interface Option {
  value: string;
  label: string;
}

export interface PresetInfo {
  id: string;
  label: string;
  blurb: string;
}

export const PRESETS: PresetInfo[] = [
  { id: 'normal', label: 'Normal', blurb: 'The game as designed – every modifier at its default.' },
  { id: 'casual', label: 'Casual', blurb: 'The most relaxed way to play: gentle fights, forgiving deaths, no raids.' },
  { id: 'easy', label: 'Easy', blurb: 'Easier fights and fewer raids.' },
  { id: 'hard', label: 'Hard', blurb: 'Tougher enemies for experienced Vikings.' },
  { id: 'hardcore', label: 'Hardcore', blurb: 'Harsh: hard fights, a heavy death penalty and no map.' },
  { id: 'immersive', label: 'Immersive', blurb: 'Exploration without crutches – no map and stricter portals.' },
  { id: 'hammer', label: 'Hammer', blurb: 'Building mode: free building and peaceful creatures.' },
];

export interface ModifierInfo {
  key: string;
  label: string;
  blurb: string;
  options: Option[];
}

export const MODIFIERS: ModifierInfo[] = [
  {
    key: 'combat',
    label: 'Combat',
    blurb: 'How hard enemies hit and how much health they have.',
    options: [
      { value: 'veryeasy', label: 'Very easy' },
      { value: 'easy', label: 'Easy' },
      { value: 'hard', label: 'Hard' },
      { value: 'veryhard', label: 'Very hard' },
    ],
  },
  {
    key: 'deathpenalty',
    label: 'Death penalty',
    blurb: 'What a Viking loses on death – items and skill levels.',
    options: [
      { value: 'casual', label: 'Casual – keep everything' },
      { value: 'veryeasy', label: 'Very easy' },
      { value: 'easy', label: 'Easy' },
      { value: 'hard', label: 'Hard' },
      { value: 'hardcore', label: 'Hardcore – lose everything' },
    ],
  },
  {
    key: 'resources',
    label: 'Resources',
    blurb: 'How much wood, ore and loot gathering yields.',
    options: [
      { value: 'muchless', label: 'Much less' },
      { value: 'less', label: 'Less' },
      { value: 'more', label: 'More' },
      { value: 'muchmore', label: 'Much more' },
      { value: 'most', label: 'Most' },
    ],
  },
  {
    key: 'raids',
    label: 'Raids',
    blurb: 'How often enemy events attack bases.',
    options: [
      { value: 'none', label: 'None' },
      { value: 'muchless', label: 'Much less' },
      { value: 'less', label: 'Less' },
      { value: 'more', label: 'More' },
      { value: 'muchmore', label: 'Much more' },
    ],
  },
  {
    key: 'portals',
    label: 'Portals',
    blurb: 'What may travel through portals.',
    options: [
      { value: 'casual', label: 'Casual – everything, ore too' },
      { value: 'hard', label: 'Hard – no boss portals' },
      { value: 'veryhard', label: 'Very hard – no portals' },
    ],
  },
];

export const KEYS: { key: string; label: string; blurb: string }[] = [
  { key: 'nobuildcost', label: 'No build cost', blurb: 'Building pieces cost nothing.' },
  { key: 'playerevents', label: 'Raids by player progress', blurb: 'Raids depend on each player’s own progress, not the world’s.' },
  { key: 'passivemobs', label: 'Passive enemies', blurb: 'Enemies only fight back when attacked.' },
  { key: 'nomap', label: 'No map', blurb: 'No minimap or world map – navigate by landmarks.' },
];

export function presetLabel(p: string) {
  return PRESETS.find((x) => x.id === p)?.label ?? (p || 'World’s own');
}

export function modifierLabel(key: string, value: string) {
  const m = MODIFIERS.find((x) => x.key === key);
  return m?.options.find((o) => o.value === value)?.label ?? value;
}

/** A world's stored keys (`preset combat_hard:…`, `nomap`) as readable chips. */
export function describeWorldKeys(keys: string[]): string[] {
  const out: string[] = [];
  for (const k of keys) {
    if (k.startsWith('preset ')) {
      for (const part of k.slice(7).split(':')) {
        const [key, val] = part.split('_');
        if (val && val !== 'default') out.push(`${MODIFIERS.find((m) => m.key === key)?.label ?? key}: ${modifierLabel(key, val)}`);
      }
    } else if (!k.includes(' ')) {
      out.push(KEYS.find((x) => x.key === k)?.label ?? k);
    }
  }
  return out;
}

export function secondsLabel(s: number) {
  if (s >= 86400 && s % 3600 === 0) return s % 86400 === 0 ? `${s / 86400} d` : `${(s / 86400).toFixed(1)} d`;
  if (s >= 3600) return s % 3600 === 0 ? `${s / 3600} h` : `${(s / 3600).toFixed(1)} h`;
  if (s >= 60) return s % 60 === 0 ? `${s / 60} min` : `${(s / 60).toFixed(1)} min`;
  return `${s} s`;
}

export const PRIORITIES = [
  { value: 'idle', label: 'Low' },
  { value: 'belowNormal', label: 'Below normal' },
  { value: 'normal', label: 'Normal' },
  { value: 'aboveNormal', label: 'Above normal' },
  { value: 'high', label: 'High' },
];
