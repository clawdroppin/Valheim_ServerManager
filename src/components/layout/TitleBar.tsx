import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Command, Copy, Minus, Square, X } from 'lucide-react';
import { inTauri } from '@/lib/ipc';
import { useApp } from '@/store/app';
import { Kbd } from '@/components/ui/Surface';

/** The mark: snowy peaks under the hearth sun, inside a rune ring. */
export function Logo({ className = 'size-[18px]' }: { className?: string }) {
  return (
    <svg viewBox="0 0 64 64" className={className}>
      <defs>
        <linearGradient id="vhsm-peak" x1="0.5" y1="0" x2="0.5" y2="1">
          <stop offset="0" stopColor="#e8f6fd" />
          <stop offset="0.5" stopColor="#6cc3e8" />
          <stop offset="1" stopColor="#24607d" />
        </linearGradient>
        <radialGradient id="vhsm-sun" cx="0.5" cy="0.5" r="0.5">
          <stop offset="0" stopColor="#fff1cf" />
          <stop offset="0.55" stopColor="#f0a24a" />
          <stop offset="1" stopColor="#c8641c" />
        </radialGradient>
      </defs>
      <circle cx="32" cy="32" r="28" fill="none" stroke="#f0a24a" strokeOpacity="0.45" strokeWidth="2.5" strokeDasharray="5 4" />
      <circle cx="39" cy="22" r="8" fill="url(#vhsm-sun)" />
      <path d="M8 48 L25 20 L34 34 L40 26 L56 48 Z" fill="url(#vhsm-peak)" />
      <path d="M25 20 L20.5 27.5 L25 26 L28.5 29 Z M40 26 L37 31 L40 30 L43 32.5 Z" fill="#ffffff" opacity="0.9" />
    </svg>
  );
}

export function TitleBar() {
  const [max, setMax] = useState(false);
  const setPalette = useApp((s) => s.setPalette);
  const win = inTauri() ? getCurrentWindow() : null;
  useEffect(() => {
    if (!win) return;
    win.isMaximized().then(setMax);
    const un = win.onResized(() => win.isMaximized().then(setMax));
    return () => {
      un.then((f) => f());
    };
  }, [win]);

  return (
    <div data-tauri-drag-region className="relative z-30 flex h-10 shrink-0 items-center border-b border-line bg-bg pl-3.5">
      <div data-tauri-drag-region className="flex items-center gap-2.5">
        <Logo />
        <span data-tauri-drag-region className="text-[12.5px] font-semibold tracking-tight text-fg">
          Valheim <span className="text-fg-3">Server Manager</span>
        </span>
      </div>
      <div data-tauri-drag-region className="flex flex-1 justify-center">
        <button
          onClick={() => setPalette(true)}
          className="flex h-7 w-[340px] items-center gap-2 rounded-lg border border-line bg-panel px-2.5 text-[12px] text-fg-4 transition-colors hover:border-line-strong hover:text-fg-3"
        >
          <Command className="size-3.5" />
          Search servers, actions, settings…
          <span className="ml-auto flex gap-0.5">
            <Kbd>Ctrl</Kbd>
            <Kbd>K</Kbd>
          </span>
        </button>
      </div>
      {win && (
        <div className="flex h-full">
          <button onClick={() => win.minimize()} className="flex h-full w-11 items-center justify-center text-fg-3 hover:bg-hover hover:text-fg">
            <Minus className="size-4" />
          </button>
          <button onClick={() => win.toggleMaximize()} className="flex h-full w-11 items-center justify-center text-fg-3 hover:bg-hover hover:text-fg">
            {max ? <Copy className="size-3.5 -scale-x-100" /> : <Square className="size-3.5" />}
          </button>
          <button onClick={() => win.close()} className="flex h-full w-11 items-center justify-center text-fg-3 hover:bg-err hover:text-white">
            <X className="size-4" />
          </button>
        </div>
      )}
    </div>
  );
}
