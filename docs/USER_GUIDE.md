# User guide

Everything you need to host a Valheim server with Valheim Server Manager – from the first install to rolling back a troll-flattened longhouse.

**Contents**
1. [Install the manager](#1-install-the-manager)
2. [Create your first server](#2-create-your-first-server)
3. [Let players in](#3-let-players-in)
4. [Day-to-day: start, stop, monitor](#4-day-to-day-start-stop-monitor)
5. [Server settings](#5-server-settings)
6. [World modifiers](#6-world-modifiers)
7. [Access & bans](#7-access--bans)
8. [Players](#8-players)
9. [Worlds](#9-worlds)
10. [Backups](#10-backups)
11. [Mods](#11-mods)
12. [Automation, several servers, import & export](#12-automation-several-servers-import--export)
13. [Troubleshooting](#13-troubleshooting)

## 1. Install the manager

- **Installer** (`Valheim-Server-Manager-Setup.exe`): installs for your Windows user; data lives in `%APPDATA%\com.valheimservermanager.app`.
- **Portable** (`…-Portable.zip`): unzip anywhere you can write to. Everything – settings, SteamCMD, servers, backups – stays in the `data` folder next to the exe.

Windows SmartScreen may warn on first launch because the app isn't code-signed: **More info → Run anyway**.

A server needs about **2 GB** of disk plus its worlds and backups, and **4 GB of RAM** at minimum (8 GB is comfortable, more with many mods). Valheim allows **10 players** per vanilla server.

## 2. Create your first server

Click **New server** (or press **Ctrl+N**). The wizard has four steps:

1. **Server** – a profile name, a colour, the install folder (leave empty for automatic) and the port. A free pair is picked for you: UDP **2456** and **2457** (Valheim always uses the port you pick and the next one); a second server gets 2459/2460.
2. **World & access** – the name players search for, the join password, the world name, whether the server is listed publicly, **Crossplay**, and your own player id (optional – it makes you an admin).
   - The password must be **at least 5 characters** and **not part of the server name** – the server refuses anything else on a public server (and hangs instead of exiting – the manager catches that, but it's better to get it right here).
3. **Difficulty** – Normal or one of Iron Gate's world-modifier presets (Casual, Easy, Hard, Hardcore, Immersive, Hammer).
4. **Launch** – review and download the server with SteamCMD (about 1 GB, no Steam account needed).

When the download finishes, press **Start**. A fresh world is generated and online in under a minute. The Overview shows a short checklist until everything is set up.

Already have a server? **Import server → Existing folder** adopts it. The manager reads the settings from the folder's start script (`start_headless_server.bat` or your own copy of it – name, port, world, password, crossplay, modifiers) and keeps using its save folder (`-savedir`, or the default `AppData\LocalLow\IronGate\Valheim`). Servers installed through the Steam client (Library → Tools) are detected automatically.

## 3. Let players in

In the game: **Start Game → pick a character → Join Game**. Players find the server under **Community** (search the name) or add it with **Join IP** / **Add server** using your public address and port. The Overview has copy buttons for the name, the address, the join code and the password – and **Copy invite** puts all of it into one message for Discord or a chat.

Friends on your own network can join right away. For everyone else there are two ways:

- **Crossplay** (Server Settings): players connect through a PlayFab relay – **no port forwarding needed** – and Xbox, Game Pass and Microsoft Store players can join too. The Overview shows the 6-digit **join code** once the server has registered it. Note: with Crossplay, a LAN address doesn't work – use the code or your public IP.
- **Steam backend** (Crossplay off): forward **UDP 2456 and 2457** (or your ports). **Port Forwarding** shows exactly what to forward; *Forward ports automatically* uses UPnP, otherwise follow the router steps.
- **Windows Firewall:** click *Allow through firewall* (one admin prompt). If you once dismissed Windows' "allow access?" popup, Windows created a hidden **Block** rule – the button removes it too.
- **Is it listed?** *Ask Steam if it's listed* checks Steam's server list, which is what the Community tab shows. The manager also asks by itself three minutes after every start of a public server and warns you if it's missing. Double NAT / CGNAT are detected and explained.

## 4. Day-to-day: start, stop, monitor

- **Start / Stop / Restart** live in the header of every server. **Stop** asks the server to save and shut down (about 4 seconds) and force-closes only after the timeout. The ⋯ menu has **Force stop**, console window, update/validate and folder shortcuts.
- The tray icon has **Start all / Stop all servers**.
- Servers **keep running when you close the manager**. With "Keep running in the tray" (default) the manager keeps watching them; when you reopen it, it re-attaches to anything still running.
- **Overview:** join details, live CPU/memory/players/disk, recent console, version (and whether mods are loaded), the active world with its in-game day, next schedule, last backup and an activity timeline.
- **Performance:** live charts (1 m – 1 h), and **24 h / 7 d / 30 d history** of players, CPU, memory and disk.
- **Console:** the live server log with filters, search and previous sessions (plus BepInEx's log when mods run). Engine chatter (shader warnings, world-generation statistics) is hidden by default. The Valheim server has no command console of its own – admin commands are typed in-game (F5); the console footer lists them for copying.
- **Ctrl+K** opens the command palette; **Ctrl+Tab** switches tabs; **Ctrl+S** saves.

## 5. Server settings

Valheim has no settings file – every option is passed on the command line when the server starts, so changes made while it runs apply at the next restart (the header offers **Restart to apply changes**).

- **Listing:** server name, public listing, Crossplay (and an instance id when several crossplay servers share a port).
- **Access:** the join password, checked live against the server's rules. The password is stored encrypted by the manager; because Valheim only accepts it on the command line, it is masked wherever the manager shows that line.
- **Network:** the port (the next one is used too) with a free-port picker.
- **World & saves:** the world name (a new name generates a fresh world), how often the world saves (default 30 minutes; it also saves on every clean stop), and the server's own world backups – how many it keeps, when the first is taken and the interval of the later ones.
- **Process & Launch:** the save folder (`-savedir`), console window, priority, CPU cores and extra arguments, plus the full command line.

## 6. World modifiers

The same options as the game's world modifier screen:

- **Preset** – Normal, Casual, Easy, Hard, Hardcore, Immersive or Hammer.
- **Modifiers** – combat, death penalty, resources, raids and portals; each overrides the preset.
- **Rules** – no build cost, raids by player progress, passive enemies, no map.

The server writes these into the world at every start. **Keep the world's own** passes nothing, so a world keeps the modifiers it was created with (useful for a world you brought from single player – the page shows what it has). **Save as preset** stores the current set under a name for any of your servers.

## 7. Access & bans

Three files in the save folder, one player id per line – the same files admins change in-game with `ban` and `unban`:

- **Admins** (`adminlist.txt`) can use the in-game console (F5): `kick`, `ban`, `unban`, `banned`, `save` and more.
- **Banned** (`bannedlist.txt`) can't connect.
- **Allow list** (`permittedlist.txt`) – while anyone is on it, **only** those players can join. Make sure your own id is on it.

Ids are `Platform_UserId`: `Steam_76561198…` for Steam players, `Xbox_…` for crossplay players. You can paste a plain SteamID64 – the manager writes it in the right form. Players see their own id in-game with **F2**; for Steam players it is also the number in their profile URL. The manager edits single lines and keeps the files' comments; **Config Files** shows the raw files with 30 versions of history.

## 8. Players

- **Online now:** character names and ids from the server log, how long they've been on and how often they died this session. The count comes from the server itself (Steam query on port + 1). Someone who is still connecting shows as "Connecting…" until their character spawns – a wrong password or a cancelled join never becomes a session.
- **Admin / Ban** buttons write the lists. **Kick…** copies `kick <name>` for the in-game console – Valheim has no remote console.
- **This session** lists joins, leaves and deaths; **History** keeps everyone who has played, with sessions and total play time (stored in the manager's own database).

## 9. Worlds

Each world is a folder in `worlds_local`. For every world the manager shows its **seed** (click to copy, or **Map** to open it in the community map viewer), the **in-game day** at the last save, its modifiers and size. Copies, exports and backups always take the world's last complete save, so they're safe while the server runs.

- **Active world:** export it (e.g. into your game's `worlds_local` to play it single-player), check its saves, duplicate, rename, reset.
- **Roll back:** the server keeps its own backups next to the world (Server Settings → Server backups). **Roll back to this** makes a zip backup, moves the current world to `_quarantine` and puts the backup in its place.
- **Check saves** unpacks the current save and every server backup to find damaged ones before the server tries to load them.
- **Other worlds:** switch the server to one (applies at the next start), start a brand-new world under a new name, export, rename, remove (moves to `_quarantine`).
- **Bring a world from the game:** worlds from this PC's game (`AppData\LocalLow\IronGate\Valheim\worlds_local`, Steam Cloud) are listed; any other folder works too. New (folder) and old (`.fwl` + `.db`) world formats are both supported – the server converts old ones on first load.
- **Renames are safe:** a world's name is also stored inside its save; the manager rewrites it, so the server keeps loading the same world.

Characters live on the players' PCs, so switching or resetting a world never touches anyone's character, inventory or skills.

## 10. Backups

The manager's backups are zips of every world plus the three permission lists. They are made on an interval while the server runs, after every stop and before updates (Automation), and before every rollback, restore, rename and reset. Manual backups are never pruned.

**Restore** puts the worlds from the backup back (the current ones go to `_quarantine` first) and, optionally, the permission lists. **Verify** reads every file and unpacks the world data. The backup folder can be moved to another drive under App settings.

## 11. Mods

Server-side mods use **BepInEx**, the mod loader almost every Valheim mod depends on, and come from **Thunderstore**, the community mod site.

- **Install a mod:** paste its Thunderstore page URL or `Namespace-Name` (e.g. `ValheimModding-Jotunn`). BepInEx and all dependencies are installed automatically.
- **From file…** installs a mod zip you downloaded or a single plugin DLL.
- **Installed mods:** switch each on or off (disabled mods move to `plugins_disabled`), remove (recoverable from `_mods-removed`), open its Thunderstore page, **Check for updates** and update.
- **Load mods** switches BepInEx off for the next start without uninstalling anything – handy to test whether a problem comes from a mod.
- **Config Files** lists every mod's `.cfg` with syntax highlighting and history. Your config is never overwritten by a mod update.
- **BepInEx log** shows the loader's output from the last start – plugin errors show up there and in Network & Health.
- **Mod list for players** copies every enabled mod with its version and Thunderstore link, so your players can install the same set (e.g. with r2modman).

Mods change files the server has loaded, so install, update, switch and remove them while the server is **stopped**. Most gameplay mods must be installed on every player's game too, in the same version; mods marked *server-side only* don't need that. After a Valheim update, mods often need an update as well.

## 12. Automation, several servers, import & export

- **Automation:** daily or weekly schedules (restart, backup, update, stop, start), automatic restart after a crash (paused after repeated crashes or a start-up error, with the reason), update checks and auto-update that can **wait until nobody is online**, stop when empty, restart on memory or uptime limits, start with Windows, backups.
- **Notifications:** Windows toasts and a Discord/Slack webhook (App settings) for crashes, updates, players joining and more.
- **Several servers:** each server is a tab; ports are picked so they never collide. **Duplicate** copies a server (optionally with its worlds).
- **Server packs** (`.valheim`): export a profile with its permission lists and, optionally, its worlds and password, and import it on another PC.

## 13. Troubleshooting

| Problem | What to do |
|---|---|
| The server isn't in the Community list | Network & Health → Steam server list. Allow the firewall, forward UDP 2456–2457, or turn on Crossplay. It can take a few minutes after start. |
| "Incompatible version" | Update the server (Install & Files or the ⋯ menu). With mods, players and server need the same mod versions too. |
| The server stops right after starting | Read the error in the toast / Network & Health – most often a password the server refuses. Auto-restart pauses until you fix it. |
| Xbox / Game Pass friends can't join | Turn on Crossplay and give them the join code. |
| Friends can't connect with my LAN IP | With Crossplay that doesn't work – use the join code or the public IP. |
| Mods don't load | Mods → BepInEx log. Check *Load mods* is on and update mods after a Valheim update. |
| A world is broken after a crash | Worlds → Check saves, then roll back to the newest intact backup. |
| "Stop the server first" | Rollbacks, renames, restores, resets and mod changes need the server stopped. |
| Admin commands don't work in-game | Make sure your id is on the admin list (Steam_7656… or your Xbox id from F2) – the server picks up changes to the list. |
