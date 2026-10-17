# AGENTS.md – hand-off guide for AI assistants and contributors

This is the hand-off document for **Valheim Server Manager**, a Windows desktop app that runs *Valheim* dedicated servers. Read it fully before changing code. `CLAUDE.md` points here. User-facing documentation lives in [`docs/USER_GUIDE.md`](docs/USER_GUIDE.md); keep it in sync when behaviour changes.

The app shares its architecture with the sibling managers in `O:\ServerManagers` (it started as a copy of the V Rising manager, itself a copy of the Enshrouded one); fixes to shared plumbing (supervisor loop, SteamCMD, backups, UPnP, secrets, UI kit) usually apply to all of them.

## 0. Repository, branches, website

- GitHub: **<https://github.com/clawdroppin/Valheim_ServerManager>**
- **`release`** – the development line everything is pushed to (the default branch; CI `ci.yml` runs on pushes to `release` and `main` and on pull requests).
- **`gh-pages`** – the project website (<https://clawdroppin.github.io/Valheim_ServerManager/>). Its source is `site/index.html` (self-contained, no build step); `node scripts/build-site.mjs` assembles it with `docs/screenshots` and the icon into `dist-site/` (git-ignored), whose contents are the whole `gh-pages` branch (plus `.nojekyll`). Publish: build, then commit `dist-site/` as the tree of `gh-pages` (an orphan branch – see §5).
- Releases: push a tag `vX.Y.Z` → `.github/workflows/release.yml` builds on `windows-latest` and publishes the installer, MSI and portable zip (§5).

### Before you start – the 60-second version

1. Valheim has **no config file**. Every setting is a command-line switch built by `server::build_args` – including the join password (§3.2). The only files the manager shares with the server are the three permission lists; edit single lines, never whole files (§4).
2. The world's name is **stored inside the world** (`.fwl2`). Copying a world folder under a new name without rewriting it splits the world (§3.8).
3. A bad password makes the server hang half-started – it never exits by itself (§3.5).
4. Every new backend command goes into `commands.rs` → `lib.rs` → `src/lib/ipc.ts` → `src/lib/devMock.ts` (§7).
5. Before you hand off: `cargo check` (no warnings), `cargo test --lib`, `npx tsc -p tsconfig.json --noEmit`. If you changed behaviour, update `docs/USER_GUIDE.md` and `CHANGELOG.md`.

## 1. Stack & commands

| Layer | Tech |
|---|---|
| Shell | Tauri 2: main window created in code, tray icon, single-instance, autostart and notification plugins |
| Backend | Rust 2021, tokio, reqwest, sysinfo, rusqlite (bundled), zip, flate2, regex, igd-next (UPnP), windows-sys (console, DPAPI) |
| Frontend | React 19, TypeScript 7, Vite 8 (dev port **1470**), Tailwind CSS 4, Motion, Zustand, CodeMirror 6, cmdk, lucide-react |

```bash
npm install
npm run tauri dev                      # full app, hot reload
npm run dev                            # UI only in a browser with the MOCK backend (src/lib/devMock.ts)
npx vite --port 1480                   # same, on another port (the .claude "ui-preview" config)
npx tsc -p tsconfig.json --noEmit      # typecheck
cd src-tauri && cargo check            # must stay warning-free (CI uses -D warnings)
cd src-tauri && cargo test --lib       # unit tests (config, lists, worlds, mods, A2S, log parsing, backups, history, DPAPI)
npm run build:all                      # NSIS + MSI + portable zip (local)
SHOTS_URL=http://localhost:1480/ node scripts/screenshots.mjs   # regenerate docs/screenshots (needs a dev server)
```

**Testing the real app.** `npx tauri build --debug --no-bundle`, put a `portable.txt` next to `src-tauri/target/debug/valheim-server-manager.exe` (keeps its data in `target/debug/data`), start it with `VHSM_CDP_PORT=9444` and drive the WebView over the Chrome DevTools Protocol (`window.__TAURI_INTERNALS__.invoke('command', args)`). The CDP hook is compiled into debug builds only. Don't use `tauri dev` for this – its rebuild kills the process tree, including servers you started.

**Real server installs** for testing go in the git-ignored `.test-server/` folder (SteamCMD: `+force_install_dir <abs path> +login anonymous +app_update 896660 validate +quit`; a fresh SteamCMD often fails the first time with "Missing configuration" – run it again).

## 2. Repository map

```
src-tauri/src/
  main.rs            entry; helper mode `--send-ctrl-c <pid>` (graceful stop, §3)
  lib.rs             setup: data dir, orphaned-WebView recovery, window, tray, plugins, command list
  commands.rs        every #[tauri::command] (thin wrappers; add new ones to lib.rs + src/lib/ipc.ts + devMock.ts)
  state.rs           AppState, per-server Runtime, TaskReporter, notify()/notify_kind() (timeline + toast + webhook), safe JSON IO
  models.rs          persisted & event types (camelCase); SERVER_APP_ID, PRESETS/MODIFIERS/SET_KEYS, ServerConfig, paths
  paths.rs           installed vs portable data dir, path rebasing
  core/
    config.rs        permission lists (comment-preserving line edits, Platform_Id normalisation), validation that
                     mirrors the server's own rules, -preset/-modifier/-setkey args, start-script import
    server.rs        command line + env, log rotation, start / stop / restart / update
    supervisor.rs    single loop: process tracking/adoption, telemetry + per-minute history, log tail & parsing
                     (joins, characters, deaths, join code, BepInEx), A2S polling, crash restart, schedules,
                     wait-for-empty, idle stop, interval backups, update checks
    worlds.rs        world folders + legacy .fwl/.db pairs, .fwl2 parsing (seed, modifiers) and renaming, world day
                     from .db2, server-backup rollback, import/export/duplicate/rename/reset, integrity check
    mods.rs          BepInExPack_Valheim install, Thunderstore packages + dependencies, enable/disable, updates,
                     mod configs, mods on/off (winhttp.dll proxy)
    backup.rs        zip backups (worlds + lists), restore, verify (gunzips .db2), retention, .valheim packs
    presets.rs       the user's own world-modifier presets (<app data>\rule-presets.json), shared by all servers
    a2s.rs           Steam A2S INFO / PLAYER (player count, names, version keywords)
    history.rs       manager.db (SQLite): metrics, event timeline, player sessions (with SteamID), builds
    secrets.rs       DPAPI encryption of the join password, branch password and webhook URL
    proc.rs          detached CreateProcessW launch (own hidden console, custom environment), console break,
                     priority/affinity, console visibility, UDP port owners, is_within(), orphaned-WebView cleanup
    steamcmd.rs      bootstrap, app_update (beta branches), build ids (api.steamcmd.net, then SteamCMD), Steam roots
    network.rs       firewall state/rules, IPs, gateway, port suggestions, Steam listing
    upnp.rs          UPnP IGD discovery, port mapping, router info, WAN address classification
    diagnostics.rs   health checks with fix ids
src/
  store/app.ts       Zustand store: instances, statuses, tabs, tasks, toasts, dialogs; onlineCount()
  lib/live.ts        ring buffers for telemetry & console, lineLevel()/isNoise() for Valheim log lines
  lib/ipc.ts         typed commands + event listen (mock-aware)   lib/types.ts mirrors models.rs
  lib/defaults.ts    default ServerConfig (mirrors ServerConfig::default)   lib/paths.ts savedir resolution
  data/gameplay.ts   world-modifier presets, modifiers and keys with plain-language labels
  features/server/   Workspace (header, nav, shared Save bar), draft.ts, views/* (Mods.tsx is Valheim-only)
  features/dialogs/  new-server wizard, import/export/clone/delete, app settings
site/                index.html – the GitHub Pages landing page (self-contained; screenshots come from docs/)
scripts/             build-portable.mjs, screenshots.mjs, build-site.mjs
.github/workflows/   ci.yml (check on push/PR), release.yml (tag v* -> GitHub Release)
```

## 3. Verified facts about the server

Tested against a real install (Steam build **25909431**, `Valheim version: 1.0.20 (network version 41)`, Unity 6000.0.75f1, October 2026). Re-verify after big game updates. Strings marked *binary* come from `valheim_server_Data\Managed\assembly_valheim.dll` (UTF-16 user strings) and were not yet seen with a real client.

1. **AppID 896660** installs anonymously (~1 GB download, ~2 GB on disk). The game client is **892970** (Steam master list `appid`, `steam_appid.txt` in the server folder, the `SteamAppId` environment variable the shipped `start_headless_server.bat` sets – the log confirms `Using environment steamid 892970`). Iron Gate's manual (`Valheim Dedicated Server Manual.pdf`) ships with the server – read it after updates.
2. **Command line only.** `valheim_server.exe -nographics -batchmode -name "N" -port P -world "W" [-password "X"] -public 1|0 [-crossplay] [-instanceid I] -savedir "D" -logFile "L" -saveinterval S -backups B -backupshort S -backuplong S [-preset p] [-modifier k v]… [-setkey k]…`. There is no settings file – the password is therefore visible to other processes on the PC; `launch_line` masks it for display and secrets are DPAPI-sealed in the manager's own files. `-resetmodifiers` exists (binary), unused.
3. **Ports:** the server binds UDP **port** and **port + 1** (netstat-verified: 2456/2457 plus two random ones). With `-crossplay` it binds port + 1 and random ones; players connect through PlayFab's relay. Second server: 2459 (`suggest_ports` steps by 3, the 2456–2458 range the official script mentions).
4. **Graceful stop = Ctrl+Break** via the helper (`--send-ctrl-c`). Log: `Game - OnApplicationQuit` → `World save (1/5)…(5/5) done` → `ZNet OnDestroy` → exit in ~4 s. The manual says Ctrl+C; Break is used because a process can inherit "ignore Ctrl+C".
5. **Password rules:** at least 5 characters and not part of the name (manual). On a **public** server a short password logs `Error bad password:The password is too short` and the server **keeps running half-started** with `NullReferenceException` spam. The supervisor kills it and pauses auto-restart (`Runtime::fatal`). An unlisted server (`-public 0`) accepted `-password abc` (verified). No password works (open server). `config::validate` mirrors these rules (error when public, warning when unlisted) and blocks starting.
6. **Log** (`-logFile`) is truncated on every start; `server::start` moves the old one to `logs\backup\valheim_server_<time>.log` (last 20). Lines: `MM/dd/yyyy HH:mm:ss: …` (Unity lines without a stamp in between). Session signature: the `Setting -savedir to:` line. Parsed (unit-tested in `supervisor.rs`):
   - version `Valheim version: 1.0.20 (network version 41)` · Steam id `Server ID 90071992547409920` · ready `Game server connected` (≈7–25 s; a new world generates locations first)
   - save `World save (5/5) done. Total time [35ms]` (preceded by `=> Save number N`) · quit `Game - OnApplicationQuit` · `isModded: False|True`
   - crossplay `Session "Name" registered with join code 748486` and every few minutes `Session "Name" with join code 748486 and IP 1.2.3.4:2456 is active with 0 player(s)`
   - players (*binary*): `Got connection SteamID 7656…` (connection – also wrong passwords), `Got character ZDOID from <Name> : <owner>:<id>` (the character spawned = a real join; `0:0` = it died), `Closing socket 7656…` (leave), `PlayFab socket with remote ID … received local Platform ID Xbox_…` (crossplay connection). The ZDOID line carries no player id – it is matched to the earliest connection still waiting for a character.
   - fatal `Error bad password:…`, `World load error, saving disabled!`, `Failed to bind socket`, `Failed to initialize platform!`
7. **A2S** answers on **port + 1** after a challenge: name, map (= server name), folder `valheim`, max 10, version `1.0.0.0`, password flag, keywords `g=1.0.20,n=41,m=…` (game version, network version, encoded modifiers – e.g. `m=0=85,1=150…` for a hard world). The supervisor polls every 10 s; the count is authoritative.
8. **Worlds** (1.0.20 format): `<savedir>\worlds_local\<World>\` holds `_main.<N>.fwl2` (meta), `_main.<N>.db2` (int32 version 42, double world time, then gzip), `_main.<N>.chunks`, `_main.<N>.ok`, and `XX_YY__<a>_<N>.chunk` files that are only rewritten when dirty. Each save bumps N and deletes the previous `_main` set (`World save … => Save number 2`). `.fwl2` layout: int32 payload length, int32 version, string name, string seed name, int32 seed, int64 uid, int32 world-gen version, bool, int32 count, strings (starting global keys – e.g. `preset combat_hard:deathpenalty_default:resources_default:raids_none:portals_default`, `nomap`, `playerdamage 85`). Strings are .NET 7-bit-length-prefixed UTF-8. The world name inside must match the folder: `worlds::set_world_name` rewrites every `_main.*.fwl2` (and legacy `.fwl`) on rename/duplicate/import.
   - Older worlds are a `<World>.fwl` + `<World>.db` pair (+ `.old`); the server converts them on load.
   - World time only runs while players are online (a fresh world stays at 2040.0 = day 1). Day = time / 1800.
   - Modifiers given on the command line are written into the world at every start (`Setting world modifier preset: hard`); without switches the world keeps its stored keys.
9. **Server backups** (`-backups/-backupshort/-backuplong`): named `<World>_backup_auto-<yyyyMMdd-HHmmss>` next to the world (*binary*: `_backup_auto-`, `_backup_restore-`, `_backup_cloud-`, format `yyyyMMdd-HHmmss`). They need real session time – none was produced in a 2-minute test even with tiny intervals (`Skipping backup. World session not long enough.`). `worlds::scan_dir` lists folders and legacy pairs with that prefix.
10. **Permission lists** live in `-savedir`: `adminlist.txt`, `bannedlist.txt`, `permittedlist.txt`, created by the server with one header line (`// List admin players ID  ONE per line`). Entries are `Platform_UserId` (manual; case sensitive) – `Steam_7656…`, `Xbox_…`. Admins' in-game `ban`/`unban` write the same files. The assembly references `GetLastWriteTime`, so the lists are most likely re-read when they change (not verified with a client). A non-empty permitted list kicks everyone not on it.
11. **Memory:** ~1.1–1.4 GB for an empty fresh world. A failed start (bad password) leaves a `worlds_local\DevWorld` stub without seed or data – `read_folder_world` hides it.
12. **Local game worlds** (import) are in `%USERPROFILE%\AppData\LocalLow\IronGate\Valheim\worlds_local` (manual: the default save path) and Steam Cloud `Steam\userdata\<id>\892970\remote\worlds` – the latter **not verified**.
13. **Mods:** with BepInEx the server log keeps its format (BepInEx adds `[Message:   BepInEx] …` lines and `isModded: True`); `BepInEx\LogOutput.log` also carries harmless `[Error  : Unity Log]` engine lines – only non-Unity errors count as mod problems. BepInExPack_Valheim 5.4.2351 (Thunderstore `denikson-BepInExPack_Valheim`) is a zip with `BepInExPack_Valheim/{winhttp.dll, doorstop_config.ini, .doorstop_version, BepInEx/core/…, BepInEx/config/BepInEx.cfg, doorstop_libs, start_*_bepinex.sh}`. Mod packages have `manifest.json` + `plugins/…`. Thunderstore API: `https://thunderstore.io/api/experimental/package/<ns>/<name>/` → `latest.{version_number, download_url, dependencies}`.
14. **Update checks:** `https://api.steamcmd.net/v1/info/896660` → `depots.branches.<branch>.buildid`, SteamCMD `app_info_print` as fallback. Iron Gate's test branch is `public-test` (password published by them).
15. **No RCON, no stdin console.** Kicking happens in-game (`kick <name>` in the F5 console as an admin); the manager offers to copy the command.

## 4. Invariants & gotchas

- **The permission lists are shared with the server and its admins.** `config::edit_list` adds/removes one line and keeps every comment; `write_text` keeps 30 versions in `_config-history` next to the file (also for mod configs).
- **World names live inside the world.** Never copy/rename a world folder without `worlds::set_world_name`. Rename = copy to `_staging` → rewrite → quarantine the old copy → move in.
- **The password is on the command line** (unavoidable); never log it unmasked (`build_args_with(inst, true)` for display) and keep it DPAPI-sealed in `instances.json`.
- **Lifecycle operations hold `AppState::op_lock(id)`** – start/stop/restart/update, mod installs, and every command that rewrites world files while requiring a stopped server (rollback, reset, rename, restore). The supervisor uses `try_lock` and skips transitions while an operation runs. Don't add per-server loops; extend the supervisor tick.
- **Fatal start-up lines kill the process** (bad password hangs it) and set `Runtime::fatal`, which stops the restart loop with the reason.
- **Players:** a connection without a character is not a session (wrong password / cancelled). Sessions start at the ZDOID line and end at `Closing socket <steamid>`; A2S reconciles names and the count.
- **Supervisor cost.** Full process discovery every 3 ticks; CPU/memory/disk only for `valheim_server.exe` PIDs; A2S every 10 s; one manager.db row per server per minute.
- **SteamCMD.** Wrap every call in `steam_lock`. Install folders must be absolute (`abs_dir`).
- **Folder containment checks use `proc::is_within`**, not string prefixes.
- **Servers outlive the manager.** The supervisor adopts any `valheim_server.exe` under a profile's install dir and recovers version, join code and online players from the current log.
- **Never lose user data.** Restores, imports, rollbacks, resets, renames and deletes quarantine first (`<savedir>\_quarantine`) and/or take a safety backup; removed mods and BepInEx go to `<server>\_mods-removed`. Deleting a server moves its `Backups` out first.
- **Mods change only while the server is stopped** (DLLs are loaded). Mod configs can be edited any time (they apply at the next start). An existing `BepInEx\config\*.cfg` is never overwritten by a mod install.
- **Only the user's actions change their router or firewall.** Firewall rules open UDP port and port + 1 only.
- **Saves are copied by their `.ok` marker.** `worlds::latest_main` picks the newest *complete* `_main.N` set and `worlds::save_files` lists only that set plus the chunk files – backups, duplicates and exports use it, so a save the server is writing at that moment is never half-copied.
- **Post-start listing check.** Three minutes after a public, Steam-backend server comes online, `spawn_listing_check` asks Steam's server list and warns when the server isn't on it.
- **Players from two sources.** The log gives SteamIDs (connection) and character names (ZDOID) separately; A2S gives names. A name A2S reports first is merged into the waiting connection (`query`), and a ZDOID line for a name A2S already knows gives that entry its ids (`tail_log`) – never two entries for one player.
- **Mock backend only in browser dev** (`!isTauri()`); extend `devMock.ts` whenever you add a command.
- **Charts.** Series `#cc7524` / `#3a9ec7` and disk-usage `#cc7524` / `#3a9ec7` / `#9b6ff0` passed the dataviz validator on the dark surface `#11171d` (all six checks). One y-axis per chart; status colours are not series colours.

## 5. Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and add a `CHANGELOG.md` entry.
2. `cargo check` (warning-free), `cargo test --lib`, `npx tsc --noEmit` and `npm run build:all` must pass locally.
3. Regenerate screenshots if pages changed (`scripts/screenshots.mjs` with the dev server on 1480), rebuild the site (`node scripts/build-site.mjs`) and update `gh-pages`:
   ```bash
   node scripts/build-site.mjs
   cd dist-site && git init -q -b gh-pages && git add -A && git commit -qm "Site" \
     && git push -f https://github.com/clawdroppin/Valheim_ServerManager.git gh-pages && cd .. && rm -rf dist-site/.git
   ```
4. Commit to `release`, then `git tag vX.Y.Z && git push origin release --tags`. `release.yml` publishes `Valheim-Server-Manager-Setup.exe`, `.msi`, `-Portable.zip` and `SHA256SUMS.txt` (the README download links point at these names). Tags with a hyphen become pre-releases. **Never move or delete a released tag.**

## 6. Ideas / not yet done

- Verify the player lines (connection, ZDOID, death, leave, PlayFab) with real Steam and Xbox clients, and the server-backup folder layout after a long session.
- Verify that the running server re-reads the permission lists (the `GetLastWriteTime` reference suggests it does).
- Decode the A2S `m=` keyword (modifiers as players see them).
- Crossplay players' leave lines (only Steam's `Closing socket` is parsed – Xbox players are removed when A2S reports fewer players).
- A Thunderstore search inside the app (the full Valheim package list is tens of MB – needs caching), modpack import (r2modman profile codes).
- Discord bot instead of a plain webhook; code signing; an auto-updater; Linux (Wine) hosts.

## 7. Recipes

**Add a backend command.** Write it in `commands.rs` (thin – logic goes in `core/*`), register it in `lib.rs`'s `generate_handler!`, add a typed wrapper to `src/lib/ipc.ts` (+ types in `src/lib/types.ts`, camelCase), and a handler in `src/lib/devMock.ts` so `npm run dev` keeps working. Long jobs: `background()` + `TaskReporter`; file work: `blocking()`.

**Add a server setting.** Field in `ServerConfig` (`models.rs`, with a serde default) → `server::build_args` → `config::validate` if a bad value can stop the server → `config::read_start_script` (import) → `src/lib/types.ts` + `defaults.ts` → `views/Settings.tsx`.

**Add a world modifier.** `models.rs` `MODIFIERS`/`SET_KEYS`/`PRESETS` (validation + args) and `src/data/gameplay.ts` (labels). Nothing else changes.

**Parse a new log line.** Capture the real line, add a regex + `Line` variant in `supervisor.rs`, handle it in `tail_log` (and `scan_log_meta` if it matters after re-adoption), and add the captured line to `parses_real_server_log_lines`. Mark lines from the binary only as such in §3.

**Change something the user sees.** Keep the UI copy plain (players, worlds, saves – not switches) and update `docs/USER_GUIDE.md`. Regenerate screenshots with `scripts/screenshots.mjs` when a page changes visibly.
