# CLAUDE.md

All project guidance for AI assistants lives in **[AGENTS.md](AGENTS.md)** – read it before making changes. User docs: [docs/USER_GUIDE.md](docs/USER_GUIDE.md).

Quick reminders:
- Steam AppID for the dedicated server is **896660** (the game is 892970 – the server runs with `SteamAppId=892970`).
- Ports: UDP **2456** (game) + **2457** (query = port + 1). Crossplay goes through PlayFab's relay.
- Valheim has **no settings file**: everything is on the command line (`server::build_args`), including the join password (masked in `launch_line`). Permission lists are `adminlist.txt` / `bannedlist.txt` / `permittedlist.txt` in `-savedir` – edit single lines only, keep the `//` header (`core/config.rs`).
- Graceful stop = Ctrl+Break to the server's own hidden console → `Game - OnApplicationQuit`, world saved, exit in ~4 s.
- Worlds are folders: `worlds_local\<World>\_main.<N>.fwl2/.db2/.chunks/.ok` + chunk files. The world's name lives inside its `.fwl2` – renames/duplicates/imports rewrite it (`worlds::rename_meta`).
- A bad password makes the server **hang**, not exit – the supervisor kills it on `Error bad password:`.
- No RCON. Mods = BepInEx + Thunderstore (`core/mods.rs`); "mods off" parks `winhttp.dll`.
- Keep `cargo check` warning-free, `cargo test --lib` and `npx tsc -p tsconfig.json --noEmit` passing.
- The mock backend (`src/lib/devMock.ts`) must only load in browser dev (`!isTauri()`).
