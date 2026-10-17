# Changelog

## 1.0.0 – 2026-10-16

First release, built on the V Rising / Enshrouded / Dragonwilds / Minecraft / ASA Server Manager architecture and verified against the real Valheim dedicated server (Steam build 25909431, Valheim 1.0.20).

- **Servers:** four-step wizard (server, world & access, difficulty, launch) with SteamCMD install; free port pairs (2456/2457, then 2459/2460 …); multi-server tabs; adopt existing installs – name, port, world, password, crossplay and modifiers are read from `start_headless_server.bat` (or your copy of it) and an existing `-savedir`; duplicate, delete, `.valheim` packs.
- **Settings:** every command-line option in plain language – name, password (checked against the server's own rules), public listing, crossplay with instance id, port, world, save interval, the server's own backup counts and intervals; the launch line is shown with the password masked.
- **World modifiers:** Iron Gate's presets as cards, combat / death penalty / resources / raids / portals, the no-map / no-build-cost / passive-enemies / player-events rules, “keep the world's own”, and your own saved modifier sets.
- **Access:** adminlist / bannedlist / permittedlist editing that keeps the server's comments, accepts SteamID64 or `Platform_Id` (Xbox too), shows names from the player history, and an editor with 30 versions of history.
- **Mods:** BepInExPack_Valheim install, Thunderstore installs by URL or name with dependencies, install from a zip or DLL, enable / disable, update checks, remove (recoverable), mod config editor, BepInEx log, and a “load mods” switch that starts the server vanilla without uninstalling.
- **Lifecycle:** graceful stop via console break (the world is saved), force stop, crash auto-restart with loop guard, start-up errors (e.g. a password the server refuses – it would hang) end the process and pause auto-restart with the reason, re-adoption after manager restarts, hidden server console, log rotation.
- **Players:** live count via Steam A2S, SteamIDs and character names from the log, deaths, session feed, player history with play time (SQLite), copy-a-kick-command for the in-game console, admin / ban from the Players page.
- **Worlds:** seed, in-game day and stored modifiers read from the save files, rollback to the server's own backups, switch / start new / duplicate / rename (the name inside the save is rewritten) / reset / remove, import from the game's worlds or any folder (new and legacy formats), integrity check that unpacks every save.
- **Backups:** every world + the permission lists, interval / on stop / before update, retention, verify (unpacks the world data), safe restore with quarantine.
- **Monitoring:** live CPU, memory, disk, network, per-core; 24 h / 7 d / 30 d history; activity timeline; live console with noise filter and previous sessions.
- **Automation:** schedules, auto-update, wait for players to leave, stop when empty, memory/uptime limits, start with Windows, tray mode, Windows notifications, Discord/Slack webhooks.
- **Network & health:** port-forwarding guide with UPnP, router detection, double-NAT/CGNAT warnings, firewall fixes incl. Block rules, Steam master-list check, A2S query tool, crossplay join code, health checks with fixes (world, mods, permissions and more).
- **Streamer mode** hides IPs, passwords, join codes and player ids.
- **Sharing:** *Copy invite* on the Overview (name, address, join code, password in one message), *Mod list for players* with Thunderstore links, open a world's seed in a map viewer.
- **Tray:** Start all / Stop all servers.
- **Safety nets:** backups, duplicates and exports copy only a world's complete save (never one being written); a "does Steam list my server?" check three minutes after every public start; players reported by the Steam query and by the log are merged instead of listed twice; Xbox players can be made admin or banned from the Players page.
- **Builds:** installer (NSIS), MSI and portable editions.
- **Website:** <https://clawdroppin.github.io/Valheim_ServerManager/>.
