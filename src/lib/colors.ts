/** Tag colours a server can wear (tab stripe, sidebar dot, cards). */
export const SERVER_COLORS: Record<string, { label: string; hex: string }> = {
  fjord: { label: 'Fjord', hex: '#4fb3d9' },
  ember: { label: 'Ember', hex: '#f0a24a' },
  moss: { label: 'Black Forest', hex: '#7fb24a' },
  gold: { label: 'Gold', hex: '#e9c46a' },
  rune: { label: 'Rune', hex: '#a78bfa' },
  blood: { label: 'Bonemass', hex: '#e5485a' },
  frost: { label: 'Frost', hex: '#a5e3f7' },
  stone: { label: 'Stone', hex: '#94a3b8' },
};

export function serverColor(c: string | undefined) {
  return SERVER_COLORS[c ?? 'fjord']?.hex ?? SERVER_COLORS.fjord.hex;
}
