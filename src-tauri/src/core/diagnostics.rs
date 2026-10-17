//! Health checks for one server, each with an optional one-click fix the UI can trigger.

use super::{config, mods, network, proc, steamcmd, worlds};
use crate::models::*;
use crate::state::AppState;

fn check(id: &str, title: &str, level: &str, detail: impl Into<String>, fix: &str) -> DiagCheck {
    DiagCheck { id: id.into(), title: title.into(), level: level.into(), detail: detail.into(), fix: fix.into() }
}

pub async fn run(state: &AppState, inst: &ServerInstance) -> Vec<DiagCheck> {
    let mut out = vec![];
    let installed = inst.is_installed();

    // Files
    if installed {
        out.push(check("install", "Server files", "ok", format!("{}", inst.server_exe().display()), ""));
        match steamcmd::manifest_state_flags(&inst.install_dir) {
            Some(4) | None => {}
            Some(f) => out.push(check(
                "manifest",
                "Interrupted update",
                "warn",
                format!("Steam reports state flags {f} – a download was interrupted. Validate the files."),
                "validate",
            )),
        }
    } else {
        out.push(check("install", "Server files", "error", "The dedicated server isn't installed in this folder yet.", "install"));
    }

    // Settings (validation – the same rules the server applies to its command line)
    let issues = config::validate(&inst.config);
    let errors: Vec<&config::Issue> = issues.iter().filter(|i| i.level == "error").collect();
    let warns: Vec<&config::Issue> = issues.iter().filter(|i| i.level == "warn").collect();
    if !errors.is_empty() {
        out.push(check("settings", "Server settings", "error", errors.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("\n"), "settings"));
    } else if !warns.is_empty() {
        out.push(check("settings", "Server settings", "warn", warns.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("\n"), "settings"));
    } else {
        out.push(check("settings", "Server settings", "ok", "Everything the manager can check looks right.", ""));
    }

    // World
    match worlds::list(inst).into_iter().find(|w| w.active) {
        Some(w) => out.push(check(
            "world",
            "World",
            "ok",
            format!(
                "“{}”{}{} – {:.1} MB.",
                w.name,
                w.meta.as_ref().map(|m| format!(", seed {}", m.seed_name)).unwrap_or_default(),
                w.day.map(|d| format!(", day {d}")).unwrap_or_default(),
                w.size as f64 / 1_048_576.0
            ),
            "",
        )),
        None => out.push(check("world", "World", "info", format!("“{}” doesn't exist yet – the server generates it on the next start.", inst.config.world.trim()), "")),
    }

    if inst.config.has_modifiers() {
        out.push(check(
            "modifiers",
            "World modifiers",
            "info",
            "This profile sets world modifiers – the server writes them into the world at every start, replacing what it had.",
            "",
        ));
    }

    // Permission lists
    let admins = config::read_list(&inst.admin_path());
    if admins.is_empty() {
        out.push(check("admins", "Admins", "info", "Nobody is an admin yet. Add your id under Access & Bans to use admin commands (kick, ban …) in the in-game console (F5).", "admins"));
    } else {
        out.push(check("admins", "Admins", "ok", format!("{} admin(s) in adminlist.txt.", admins.len()), ""));
    }
    let permitted = config::read_list(&inst.permit_path());
    if !permitted.is_empty() {
        out.push(check("permitted", "Allow list", "info", format!("Only the {} player(s) on permittedlist.txt can join – everyone else is kicked.", permitted.len()), "admins"));
    }

    // Ports
    let instances = state.instances.read().await.clone();
    let mine: Vec<u16> = inst.udp_ports();
    let clash: Vec<String> = instances.iter().filter(|o| o.id != inst.id).filter(|o| o.udp_ports().iter().any(|p| mine.contains(p))).map(|o| o.name.clone()).collect();
    if !clash.is_empty() {
        out.push(check("ports", "Port conflict", "error", format!("Ports overlap with: {}. Only one of them can run at a time.", clash.join(", ")), "ports"));
    }
    let (pids, _) = super::server::running_pids(inst);
    let udp = proc::udp_port_owners().await;
    let busy = inst.udp_ports().into_iter().find_map(|p| udp.get(&p).filter(|o| !pids.contains(o)).map(|o| (p, *o)));
    if let Some((port, pid)) = busy {
        let mut sys = sysinfo::System::new();
        let name = proc::process_name(&mut sys, pid).unwrap_or_default();
        out.push(check("portbusy", "Port in use", "error", format!("UDP {port} is held by {name} (PID {pid}). Pick another port."), "ports"));
    } else if clash.is_empty() {
        out.push(check(
            "ports",
            "Ports",
            "ok",
            format!("UDP {} (game) and {} (query) are available{}.", inst.config.port, inst.config.query_port(), if pids.is_empty() { "" } else { " (used by this server)" }),
            "",
        ));
    }
    if inst.config.crossplay {
        out.push(check("crossplay", "Crossplay", "info", "Players connect through PlayFab's relay – no port forwarding needed. They join with the join code, your public IP, or the server list (not with a LAN address).", ""));
    }

    // Firewall
    let fw = network::firewall_state(inst).await;
    if !fw.blocked_by.is_empty() {
        out.push(check(
            "firewall",
            "Windows Firewall is blocking the server",
            "error",
            format!("Inbound Block rule(s) “{}” exist for valheim_server.exe – usually from dismissing Windows’ “allow access?” prompt. Block beats Allow.", fw.blocked_by.join("”, “")),
            "firewall",
        ));
    } else if fw.ports_ok || fw.program_rule {
        out.push(check("firewall", "Windows Firewall", "ok", "An inbound rule allows the server.", ""));
    } else {
        out.push(check("firewall", "Windows Firewall", "warn", "No inbound rule for this server – players outside your PC may not reach it.", "firewall"));
    }

    // Mods
    if mods::bepinex_installed(inst) {
        let s = mods::status(inst);
        let on = s.mods.iter().filter(|m| m.enabled).count();
        if !inst.launch.mods_enabled {
            out.push(check("mods", "Mods", "info", format!("BepInEx is installed but mods are switched off – the server starts vanilla ({on} mod(s) waiting)."), "mods"));
        } else {
            // `[Error  : Unity Log]` lines are the engine's own shader/asset noise (also in vanilla) –
            // only errors raised by BepInEx or a plugin matter here.
            let errors: Vec<String> =
                mods::log_tail(inst, 400).into_iter().filter(|l| (l.starts_with("[Error") || l.starts_with("[Fatal")) && !l.contains(": Unity Log]")).take(3).collect();
            if errors.is_empty() {
                out.push(check("mods", "Mods", "ok", format!("BepInEx {} with {on} mod(s). Players need the same client-side mods.", s.bepinex_version.unwrap_or_default()), ""));
            } else {
                out.push(check("mods", "Mod errors", "warn", format!("BepInEx logged errors at the last start:\n{}", errors.join("\n")), "mods"));
            }
        }
    }

    // Memory (measured: ~1.4 GB for an empty fresh world; grows with explored land and players)
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    if total_gb < 4.0 {
        out.push(check("ram", "Memory", "warn", format!("4 GB is the minimum for a Valheim server (8 GB comfortable); this PC has {total_gb:.1} GB."), ""));
    } else {
        out.push(check("ram", "Memory", "ok", format!("{total_gb:.1} GB installed – a server uses about 1.5–4 GB."), ""));
    }

    // Disk
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let dir = proc::norm(&inst.install_dir);
    if let Some(d) = disks.iter().filter(|d| dir.starts_with(&proc::norm(d.mount_point()))).max_by_key(|d| d.mount_point().as_os_str().len()) {
        let free = d.available_space() as f64 / 1_073_741_824.0;
        let lvl = if free < 2.0 { "error" } else if free < 8.0 { "warn" } else { "ok" };
        out.push(check("disk", "Disk space", lvl, format!("{free:.1} GB free on {} (the server needs ~2 GB plus its worlds and backups; it refuses to save below ~200 MB).", d.mount_point().display()), ""));
    }

    // Updates
    let (installed_b, latest_b, public_ip) = state.with_runtime(&inst.id, |r| (r.installed_build.clone(), r.latest_build.clone(), r.public_ip.clone())).await;
    if let (Some(i), Some(l)) = (&installed_b, &latest_b) {
        if i != l {
            out.push(check("update", "Server version", "warn", format!("Build {i} is installed but {l} is live – players on the new game version can't join until you update."), "update"));
        } else {
            out.push(check("update", "Server version", "ok", format!("Up to date (build {i})."), ""));
        }
    }

    // Steam master list – the in-game server browser (Steam backend only).
    if !pids.is_empty() && inst.config.public && !inst.config.crossplay {
        let ip = match public_ip {
            Some(ip) => Some(ip),
            None => network::public_ip(&state.http).await,
        };
        if let Some(ip) = ip {
            match network::steam_listing(&state.http, &ip).await {
                Ok(list) if list.iter().any(|s| s.gameport == inst.config.port || s.gameport == inst.config.query_port()) => {
                    out.push(check("listing", "Steam server list", "ok", format!("Listed at {ip} – players can find “{}” in the community server list.", inst.config.name), ""))
                }
                Ok(_) => out.push(check(
                    "listing",
                    "Steam server list",
                    "warn",
                    format!("Steam has no Valheim server at {ip}:{} yet. It can take a few minutes after start; if it stays missing, check the firewall and router (Port Forwarding) or turn on Crossplay.", inst.config.port),
                    "ports",
                )),
                Err(e) => out.push(check("listing", "Steam server list", "info", format!("Couldn't ask Steam: {e}"), "")),
            }
        }
    } else if !inst.config.public {
        out.push(check("listing", "Server list", "info", "Not listed – players join with Join IP (and the join code with Crossplay).", "settings"));
    }

    // Last log
    if let Ok(b) = std::fs::read(inst.log_path()) {
        let text = String::from_utf8_lossy(&b);
        let bad: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("Error bad password") || l.contains("World load error") || l.contains("Failed to bind socket") || l.contains("Error saving world"))
            .take(3)
            .collect();
        if !bad.is_empty() {
            out.push(check("log", "Errors in the last log", "error", bad.join("\n"), "log"));
        }
    }
    out
}
