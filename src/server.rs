//! Copyright 2026 CallMeAlphabet (ItzAlphabet)
//!
//! Licensed under the Apache License, Version 2.0 (the "License");
//! you may not use this file except in compliance with the License.
//! You may obtain a copy of the License at
//!
//!    http://www.apache.org/licenses/LICENSE-2.0
//!
//! Unless required by applicable law or agreed to in writing, software
//! distributed under the License is distributed on an "AS IS" BASIS,
//! WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//! See the License for the specific language governing permissions and
//! limitations under the License.

use anyhow::Result;
use colored::Colorize;
use shellexpand::tilde;
use std::fs;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use crate::backup::backup_server;
use crate::config::{server_name, server_notes, server_port, server_type, server_version, set_whitelist, sm_dir, sm_file, whitelist_enabled, write_conf, write_meta};
use crate::download::{get_public_ip, lookup_player_uuid};
use crate::rcon::{rcon_available, rcon_send};
use crate::{log_base, servers_dir, sep, success, title, warn, info, prompt_line, print_banner, prompt_usize, prompt_yn, read_meta, edit_notes_interactive, subtitle, TMUX_PREFIX};

pub fn session_name(dir: &Path) -> String {
    format!("{}-{}", TMUX_PREFIX, server_name(dir).replace(&['.', ' '], "_"))
}


pub fn server_running(dir: &Path) -> bool {
    get_server_pid(dir).is_some()
}


pub fn tmux_session_exists(sess: &str) -> bool {
    Command::new("tmux")
        .args(&["has-session", "-t", sess])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}


 pub fn scan_servers() -> Vec<PathBuf> {
    let mut servers: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(servers_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                if name != "backups" && name != "LOGS" {
                    let jar_count = fs::read_dir(&path)
                        .map(|e| e.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|ext| ext == "jar").unwrap_or(false)).count())
                        .unwrap_or(0);
                    if jar_count > 0 {
                        servers.push(path);
                    } else if path.join(".mcserver.conf").exists() || sm_dir(&path).exists() {
                        servers.push(path);
                    }
                }
            }
        }
    }
    servers
}


 pub fn server_state(dir: &Path) -> &'static str {
    let jar_count = fs::read_dir(dir)
        .map(|e| e.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|ext| ext == "jar").unwrap_or(false)).count())
        .unwrap_or(0);
    if jar_count == 0 {
        if dir.join(".mcserver.conf").exists() || sm_dir(dir).exists() {
            "broken"
        } else {
            "empty"
        }
    } else if sm_dir(dir).exists() && dir.join("start.sh").exists() {
        "initialized"
    } else {
        "fresh"
    }
}


static RAM_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^([0-9]*\.?[0-9]+)\s*([a-z]*)$").unwrap());

static PORT_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new("server-port=.*").unwrap());

pub fn parse_ram_to_mib(input: &str) -> Option<f64> {
    let input = input.trim().to_lowercase();
    let caps = RAM_RE.captures(&input)?;
    let num: f64 = caps[1].parse().ok()?;
    let unit = caps.get(2).map_or("", |m| m.as_str());

    match unit {
        "" | "g" | "gi" | "gib" => Some(num * 1024.0),
        "gb" => Some(num * 1000.0),
        "m" | "mb" | "mib" => Some(num),
        "k" | "kb" | "kib" => Some(num / 1024.0),
        "t" | "tb" | "tib" => Some(num * 1024.0 * 1024.0),
        "p" | "pb" | "pib" => Some(num * 1024.0 * 1024.0 * 1024.0),
        _ => None,
    }
}


pub fn format_java_ram(mib: f64) -> String {
    if mib >= 1024.0 {
        let gib = mib / 1024.0;
        if gib.fract() < 0.001 {
            format!("{}G", gib.round() as u64)
        } else {
            format!("{:.1}G", gib)
        }
    } else if mib >= 1.0 {
        if mib.fract() < 0.001 {
            format!("{}M", mib.round() as u64)
        } else {
            format!("{:.1}M", mib)
        }
    } else {
        format!("{}M", mib)
    }
}


pub fn java_flags_for(total_mib: f64) -> String {
    let xmx_mib = total_mib.max(512.0) as u64;
    let metaspace_mib = (total_mib * 0.12).max(128.0).min(512.0) as u64;
    let direct_mib = (total_mib * 0.04).max(16.0).min(48.0) as u64;
    format!(
        "-Xmx{}m -Xms{}m -XX:MaxMetaspaceSize={}m -XX:MaxDirectMemorySize={}m -Xss256k",
        xmx_mib, xmx_mib, metaspace_mib, direct_mib
    )
}


pub fn get_ram(dir: &Path) -> String {
    if let Some(mib) = get_ram_mib(dir) {
        let gib = mib as f64 / 1024.0;
        if gib.fract() < 0.001 {
            format!("{}", gib.round() as u64)
        } else {
            format!("{:.1}", gib)
        }
    } else {
        "?".to_string()
    }
}


pub fn get_ram_mib(dir: &Path) -> Option<u64> {
    let content = fs::read_to_string(dir.join("start.sh")).ok()?;
    let cap = content.find("-Xmx")?;
    let start = cap + 4;
    let rest = &content[start..];
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let unit_str: String = rest.chars().skip_while(|c| c.is_ascii_digit() || *c == '.').take_while(|c| c.is_ascii_alphabetic()).collect();
    if !num_str.is_empty() {
        parse_ram_to_mib(&format!("{}{}", num_str, unit_str)).map(|m| m as u64)
    } else {
        None
    }
}


pub fn move_file(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dest = dest.as_ref();
    match fs::rename(src, dest) {
        Ok(_) => Ok(()),
        Err(e) if e.raw_os_error() == Some(18) => {
            if src.is_dir() {
                copy_dir_all(src, dest)?;
                fs::remove_dir_all(src)?;
            } else {
                fs::copy(src, dest)?;
                fs::remove_file(src)?;
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> Result<()> {
    let src = src.as_ref();
    let dest = dest.as_ref();
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}


pub fn run_first_time_setup(dir: &Path) -> Result<()> {
    let name = server_name(dir);

    print_banner();
    title("Add a new server");
    sep();
    println!();
    info(&format!("Running first-time setup for {}...", name.bold()));
    println!();

    let jar_name = "server.jar";

    let eula_path = dir.join("eula.txt");
    if !eula_path.exists() {
        fs::write(&eula_path, "eula=true\n")?;
        success("EULA accepted.");
    }

    info("Generating world...");

    let world_dir = dir.join("world");
    if world_dir.exists() {
        let _ = fs::remove_dir_all(&world_dir);
    }

    let mut child = Command::new("java")
        .args(&["-Xmx2G", "-Xms2G", "-jar", jar_name, "nogui"])
        .current_dir(dir)
        .spawn()?;

    let world_dat = dir.join("world/level.dat");
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(180);

    while start.elapsed() < timeout {
        if world_dat.exists() && world_dat.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    info("Stopping server...");
    let pid = child.id();
    let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };

    let wait_start = std::time::Instant::now();
    let wait_timeout = std::time::Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = status;
            break;
        }
        if wait_start.elapsed() > wait_timeout {
            let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            let _ = child.wait();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    println!();
    info("First run complete.");

    let props = dir.join("server.properties");
    if props.exists() {
        info("Configuring RCON...");
        let rcon_pass: String = rand::random::<[u8; 16]>()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();

        let content = fs::read_to_string(&props)?;
        let mut lines: Vec<String> = content
            .lines()
            .filter(|l| !l.starts_with("enable-rcon") && !l.starts_with("rcon.port") && !l.starts_with("rcon.password"))
            .map(|s| s.to_string())
            .collect();
        lines.push("enable-rcon=true".to_string());
        lines.push("rcon.port=25575".to_string());
        lines.push(format!("rcon.password={}", rcon_pass));
        fs::write(&props, format!("{}\n", lines.join("\n")))?;

        write_conf(dir, "rcon_password", &rcon_pass);
        write_conf(dir, "rcon_port", "25575");
        success("RCON configured.");
    }

    print_banner();
    title("Add a new server");
    sep();
    let ram_input = prompt_line("How much RAM (e.g. 4G, 4GiB, 4GB, 512M, 1T, etc.):");
    let ram_mib = parse_ram_to_mib(&ram_input).unwrap_or(4096.0);
    write_meta(dir, "sandbox", "ulimit");
    setup_security_networking(dir)?;

    let java_flags = java_flags_for(ram_mib);
    let start_content = format!(r#"#!/usr/bin/env bash
# fastmc — start script for {}
cd "$(dirname "$0")"
ulimit -c 0
ulimit -n 8192
exec java {} -jar server.jar nogui
"#, name, java_flags);

    fs::write(dir.join("start.sh"), &start_content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("start.sh"), std::fs::Permissions::from_mode(0o755))?;
    }

    
    if read_meta(dir, "backup_keep").is_empty() {
        write_meta(dir, "backup_keep", "5");
    }

    println!();
    success(&format!("Setup complete for {}!", name.bold()));
    println!();
    sep();
    println!("  {}", "Start the server from the main menu.".dimmed());
    sep();
    prompt_line("Press Enter to continue...");

    Ok(())
}


pub fn run_supervisor(dir: &Path) -> Result<()> {
    let name = server_name(dir);
    let log_base = log_base();
    let _ = fs::create_dir_all(&log_base);

    let log_path = || {
        let now = chrono::Local::now();
        log_base.join(now.format("%Y-%m-%d.log").to_string())
    };

    let mut crash_times: Vec<i64> = Vec::new();
    const MAX_CRASHES: i32 = 3;
    const WINDOW_SECS: i64 = 600;

    loop {
        if sm_file(dir, "stopping").exists() {
            let _ = fs::remove_file(sm_file(dir, "stopping"));
            return Ok(());
        }

        info(&format!("Starting {}...", name.bold()));
        let start_time = chrono::Local::now().timestamp();

        let mut child = Command::new("bash")
            .arg("./start.sh")
            .current_dir(dir)
            .spawn()?;

        let exit_code = match child.wait() {
            Ok(status) => status.code().unwrap_or(1),
            Err(_) => 1,
        };
        let end_time = chrono::Local::now().timestamp();
        let duration = end_time - start_time;

        if sm_file(dir, "stopping").exists() {
            let _ = fs::remove_file(sm_file(dir, "stopping"));
            return Ok(());
        }

        if exit_code == 0 {
            let _ = std::io::Write::write_all(
                &mut std::fs::OpenOptions::new().create(true).append(true).open(log_path())?,
                format!("{}  CLEAN EXIT for {} (exit 0, ran {}s)\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), name, duration).as_bytes(),
            );
            return Ok(());
        }

        let _ = std::io::Write::write_all(
            &mut std::fs::OpenOptions::new().create(true).append(true).open(log_path())?,
            format!("{}  CRASH: {}\n{}  Ran for: {}s  |  Exit code: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), name, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), duration, exit_code).as_bytes(),
        );

        let now = chrono::Local::now().timestamp();
        crash_times.retain(|&t| now - t < WINDOW_SECS);
        crash_times.push(now);

        if crash_times.len() >= MAX_CRASHES as usize {
            let _ = std::io::Write::write_all(
                &mut std::fs::OpenOptions::new().create(true).append(true).open(log_path())?,
                format!("{}  TOO MANY CRASHES — {} in {} min. Giving up.\n{}  Manual intervention required for: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), MAX_CRASHES, WINDOW_SECS / 60, chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), name).as_bytes(),
            );
            warn(&format!("TOO MANY CRASHES — {} in {} min. Giving up.", MAX_CRASHES, WINDOW_SECS / 60));
            warn(&format!("Manual intervention required for: {}", name));
            println!();
            info("The supervisor will stay alive. Attach to the tmux session to investigate.");
            println!();

            loop {
                if sm_file(dir, "stopping").exists() {
                    let _ = fs::remove_file(sm_file(dir, "stopping"));
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}



pub fn setup_security_networking(dir: &Path) -> Result<()> {
    let name = server_name(dir);
    let props = dir.join("server.properties");

    print_banner();
    title(&format!("Security & Networking — {}", name));
    sep();
    println!();

    info("Fetching your public IP address...");
    let public_ip = get_public_ip();
    if public_ip != "unknown" {
        success(&format!("Your public IP: {}", public_ip.bold().cyan()));
    } else {
        warn("Could not determine public IP (no internet access?).");
    }

    prompt_line("Press Enter to continue...");
    print_banner();

    title("Server port");
    sep();

    let srv_port: String = prompt_line("Port to run on [default: 25565]:");
    let srv_port = if srv_port.is_empty() { "25565".to_string() } else { srv_port };

    write_meta(dir, "port", &srv_port);
    let content = fs::read_to_string(&props)?;
    let new_props = if content.contains("server-port=") {
        PORT_RE.replace(&content, &format!("server-port={}", srv_port))
            .to_string()
    } else {
        format!("{}\nserver-port={}", content, srv_port)
    };
    fs::write(&props, &new_props)?;
    success(&format!("Server port set to {}", srv_port));

    write_meta(dir, "network_mode", "public");
    if public_ip != "unknown" {
        println!("   {}Players connect to: {}:{}", " ".bold(), public_ip.cyan(), srv_port);
        println!("  {}", format!("Make sure port {} (TCP) is forwarded on your router.", srv_port).dimmed());
    }

    prompt_line("Press Enter to continue...");
    print_banner();

    Ok(())
}


pub fn manage_server(dir: &Path) -> Result<()> {
    let name = server_name(dir);

    loop {
        print_banner();
        let running_badge = if server_running(dir) {
            "● RUNNING".green().bold().to_string()
        } else {
            "○ stopped".dimmed().to_string()
        };

        println!("  {} {}  {}", "Managing:".bold(), name.green().bold(), running_badge);
        let port = server_port(dir);
        let port = if port.is_empty() { "25565".to_string() } else { port };
        println!("  {} {}  {} {}  {} {}  {} {}",
            "Type:".dimmed(), server_type(dir),
            "Version:".dimmed(), server_version(dir),
            "Port:".dimmed(), port,
            "RAM:".dimmed(), format!("{}G", get_ram(dir)).dimmed()
        );
        sep();

        println!();

        let mut options: Vec<&str> = Vec::new();

        if server_state(dir) == "fresh" {
            options.push("Run first-time setup");
        }

        if server_state(dir) == "broken" {
            options.push("Recover server");
        }

        if server_state(dir) == "initialized" {
            if server_running(dir) {
                options.push("Attach to console");
                options.push("View console output");
                options.push("Resource monitor");
                options.push("Stop server gracefully");
                options.push("Force stop");
            } else {
                options.push("Start server");
            }
            options.push("Backup server");
            options.push("Manage whitelist");
            options.push("Change RAM allocation");
            options.push("Duplicate server");
        }

        options.push("Edit notes");
        options.push("Rename server");
        options.push("Delete server");
        options.push("Back to main menu");

        for (i, opt) in options.iter().enumerate() {
            let label = match *opt {
                "Attach to console" | "View console output" => format!("{}. {}", (i+1).to_string().cyan().bold(), opt.green()),
                "Stop server gracefully" => format!("{}. {}", (i+1).to_string().cyan().bold(), opt.yellow()),
                "Force stop" | "Delete server" => format!("{}. {}", (i+1).to_string().cyan().bold(), opt.red()),
                "Change RAM allocation" => {
                    format!("{}. {} (currently {}G)", (i+1).to_string().cyan().bold(), opt, get_ram(dir))
                }
                _ => format!("{}. {}", (i+1).to_string().cyan().bold(), opt),
            };
            println!("  {}", label);
        }

        let choice = prompt_usize("");

        if choice == 0 || choice > options.len() {
            warn("Pick an option.");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }

        match options[choice - 1] {
            "Back to main menu" => return Ok(()),
            "Run first-time setup" => {
                run_first_time_setup(dir)?;
                return Ok(());
            }
             "Recover server" => {
                println!();
                info("Recovering server — provide the path to the server JAR.");
                println!();

                loop {
                    let raw = prompt_line("Full path to the JAR (or Ctrl+C to cancel):");
                    let trimmed = raw.trim();
                    let unquoted = trimmed
                        .strip_prefix("'")
                        .and_then(|s| s.strip_suffix("'"))
                        .or_else(|| trimmed.strip_prefix("\"").and_then(|s| s.strip_suffix("\"")))
                        .unwrap_or(trimmed);
                    let expanded = tilde(unquoted).into_owned();
                    let jar_src_path = PathBuf::from(&expanded);
                    if jar_src_path.exists() {
                        let dest_jar = dir.join("server.jar");
                        if jar_src_path != dest_jar {
                            let _ = std::fs::copy(&jar_src_path, &dest_jar);
                        }
                        success(&format!("JAR restored to {}", dest_jar.display()));
                        break;
                    }
                    warn(&format!("File not found: {}", expanded));
                }

                if !dir.join("start.sh").exists() {
                    let start_sh = dir.join("start.sh");
                    let _ = std::fs::write(&start_sh, "#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\"\nexec java -Xmx2G -Xms2G -jar server.jar nogui\n");
                }

                prompt_line("Press Enter to continue...");
                continue;
            }
            "Start server" => {
                println!();
                let sess = session_name(dir);

                if server_running(dir) {
                    success(&format!("{} is already running in tmux session '{}'", name.bold(), sess));
                    println!("  {}Attach: tmux attach -t {}", " ".dimmed(), sess);
                    prompt_line("Press Enter to continue...");
                    continue;
                }

                if tmux_session_exists(&sess) {
                    let _ = Command::new("tmux")
                        .args(&["kill-session", "-t", &sess])
                        .status();
                }

                let world_dir = dir.join("world");
                if world_dir.exists() {
                    let level_dat = world_dir.join("level.dat");
                    let dim_settings = world_dir.join("dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat");
                    if level_dat.exists() && !dim_settings.exists() {
                        warn("Incomplete world detected, removing...");
                        let _ = fs::remove_dir_all(&world_dir);
                    }
                }

                info(&format!("Starting {} in tmux session '{}'...", name.bold(), sess));

                let fastmc_bin = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "fastmc".to_string());

                let dir_str = dir.to_string_lossy();
                let tmux_cmd = format!(
                    "tmux new-session -d -s {} -c '{}' {} supervisor '{}'",
                    sess,
                    dir_str.replace("'", "'\\''"),
                    fastmc_bin,
                    dir_str.replace("'", "'\\''")
                );
                let _ = Command::new("sh")
                    .args(&["-c", &tmux_cmd])
                    .status();

                std::thread::sleep(std::time::Duration::from_secs(10));
                if server_running(dir) {
                    success("Server started!");
                    println!("  {}Attach: tmux attach -t {}", " ".dimmed(), sess);
                } else {
                    warn("Session exited immediately. Check logs/ for errors.");
                }
                prompt_line("Press Enter to continue...");
            }
            "Attach to console" => {
                let sess = session_name(dir);
                info(&format!("Attaching to {}...", sess.cyan().bold()));
                let inside_tmux = std::env::var("TMUX").is_ok();
                if inside_tmux {
                    info("You are inside tmux. To attach, press Ctrl+B then : and run:");
                    println!("  {}switch-client -t {}", "  ".dimmed(), sess);
                    prompt_line("Press Enter when done...");
                } else {
                    let _ = Command::new("tmux")
                        .args(&["attach-session", "-t", &sess])
                        .status();
                }
            }
            "View console output" => {
                let sess = session_name(dir);
                print_banner();
                title(&format!("Console output — {}", name));
                sep();
                println!();
                title("Last 40 lines");
                sep();
                println!();
                let output = Command::new("tmux")
                    .args(&["capture-pane", "-p", "-J", "-t", &sess, "-S", "-40"])
                    .output();
                if let Ok(out) = output {
                    let logs = String::from_utf8_lossy(&out.stdout);
                    println!("{}", logs.trim_end());
                }
                println!();
                sep();
                prompt_line("Press Enter to continue...");
            }
            "Resource monitor" => {
                resource_monitor(dir)?;
            }
            "Stop server gracefully" => {
                println!();
                let sess = session_name(dir);
                fs::write(sm_file(dir, "stopping"), "")?;

                let _ = Command::new("tmux")
                    .args(&["send-keys", "-t", &sess, "stop", "Enter"])
                    .status();

                success("Stop command sent.");
                prompt_line("Press Enter to continue...");
            }
            "Force stop" => {
                println!();
                warn("This will immediately kill the server process without clean shutdown");
                let confirm = prompt_line("Type 'yes' to confirm:");

                if confirm == "yes" {
                    fs::write(sm_file(dir, "stopping"), "")?;
                    let sess = session_name(dir);
                    let _ = Command::new("tmux").args(&["kill-session", "-t", &sess]).status();
                    success("Server force-stopped.");
                }
                prompt_line("Press Enter to continue...");
            }
            "Backup server" => {
                backup_server(dir)?;
            }
            "Manage whitelist" => {
                manage_whitelist(dir)?;
            }
             "Change RAM allocation" => {
                let cur_gib = get_ram(dir);
                let new_ram_input = prompt_line(&format!("New RAM (currently {}G) — e.g. 4G, 4GiB, 4GB, 512M, 1T, etc.:", cur_gib));
                let new_ram_mib = parse_ram_to_mib(&new_ram_input).unwrap_or(4096.0);
                let new_ram_spec = format_java_ram(new_ram_mib);

                let java_flags = java_flags_for(new_ram_mib);

                let start_content = format!(r#"#!/usr/bin/env bash
cd "$(dirname "$0")"
ulimit -c 0
ulimit -n 8192
exec java {} -jar server.jar nogui
"#, java_flags);

                fs::write(dir.join("start.sh"), &start_content)?;
                success(&format!("RAM updated to {}.", new_ram_spec));
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            "Duplicate server" => {
                duplicate_server(dir)?;
                return Ok(());
            }
            "Edit notes" => {
                let cur_notes = server_notes(dir);

                print_banner();
                title(&format!("Edit notes — {}", name));
                sep();
                println!();

                let new_notes = edit_notes_interactive(&cur_notes);
                let trimmed = new_notes.trim();
                write_meta(dir, "notes", trimmed);
                if !trimmed.is_empty() {
                    success("Notes saved.");
                } else {
                    success("Notes cleared.");
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            "Rename server" => {
                if server_running(dir) {
                    println!();
                    warn("Stop the server before renaming.");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
                let new_name: String = prompt_line(&format!("New name for '{}':", name));

                let new_name = new_name.replace(' ', "-");
                if !new_name.is_empty() && !servers_dir().join(&new_name).exists() {
                    let new_dir = servers_dir().join(&new_name);
                    move_file(dir, &new_dir)?;
                    
                    success(&format!("'{}' → '{}'", name, new_name));
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                return Ok(());
            }
            "Delete server" => {
                if server_running(dir) {
                    println!();
                    warn("Stop the server before deleting.");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
                println!();
                warn(&format!("This will permanently delete {} and ALL its files.", name.bold()));
                let confirm = prompt_line("Type the server name to confirm:");

                if confirm == name {
                    let delete_confirm = prompt_line("Delete? [No/yes]:");
                    if delete_confirm.trim().eq_ignore_ascii_case("yes") {
                        fs::remove_dir_all(dir)?;
                        success(&format!("Server '{}' deleted.", name));
                    } else {
                        warn("Deletion cancelled.");
                    }
                } else {
                    warn("Name didn't match. Deletion cancelled.");
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                return Ok(());
            }
            _ => {}
        }
    }
}


pub fn view_crash_logs() -> Result<()> {
    print_banner();
    title("Crash Logs");
    subtitle(&log_base().display().to_string());
    sep();
    println!();

    if let Ok(entries) = fs::read_dir(log_base()) {
        let logs: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
            .collect();

        if logs.is_empty() {
            println!("  {}", "No crash logs found — servers have been well-behaved!".dimmed());
            sep();
            prompt_line("Press Enter to continue...");
            return Ok(());
        }

        for (i, log) in logs.iter().enumerate() {
            let fname = log.file_name().to_string_lossy().to_string();
            let size = fs::metadata(log.path())
                .map(|m| m.len())
                .map(|b| format!("{:.1} KB", b as f64 / 1024.0))
                .unwrap_or_else(|_| "?".to_string());
            println!("  {}. {}  ({})", (i+1).to_string().cyan().bold(), fname, size.dimmed());
        }
        println!();
        sep();
        println!("  {}", "Back [b]".cyan().bold());

        let choice = prompt_usize("");

        if choice > 0 && choice <= logs.len() {
            let content = fs::read_to_string(logs[choice - 1].path())?;
            println!();
            sep();
            println!("{}", content);
            sep();
            prompt_line("Press Enter to continue...");
        }
    }

    Ok(())
}


pub fn duplicate_server(src_dir: &Path) -> Result<()> {
    let src_name = server_name(src_dir);

    print_banner();
    title(&format!("Duplicate server — {}", src_name));
    sep();

    let new_name = prompt_line("New server name:");
    let new_name = new_name.replace(' ', "-");

    if new_name.is_empty() || servers_dir().join(&new_name).exists() {
        warn(&format!("Invalid name or '{}' already exists.", new_name));
        sep();
        prompt_line("Press Enter to continue...");
        return Ok(());
    }

    let copy_world = prompt_yn("Copy world data too? [Y/n]:", true);

    let dest_dir = servers_dir().join(&new_name);

    if copy_world {
        fs::create_dir_all(&dest_dir)?;
        copy_recursively(src_dir, &dest_dir)?;
    } else {
        fs::create_dir_all(&dest_dir)?;
        copy_without_worlds(src_dir, &dest_dir)?;
    }

    println!();
    success(&format!("Duplicated to {}", dest_dir.display()));
    sep();
    prompt_line("Press Enter to continue...");
    Ok(())
}


pub fn copy_recursively(src: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_recursively(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}


pub fn copy_without_worlds(src: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if ["world", "world_nether", "world_the_end", "logs"].contains(&name.as_str()) {
            continue;
        }
        let dest_path = dest.join(name);
        if entry.path().is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_without_worlds(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}


pub fn start_all_servers(servers: &[PathBuf]) -> Result<()> {
    print_banner();
    title("Start all servers");
    sep();
    println!();

    let mut started = 0usize;
    for dir in servers {
        if server_state(dir) == "initialized" {
            let name = server_name(dir);
            let sess = session_name(dir);

            if server_running(dir) {
                info(&format!("{} already running", name.dimmed()));
                continue;
            }

            if tmux_session_exists(&sess) {
                let _ = Command::new("tmux")
                    .args(&["kill-session", "-t", &sess])
                    .status();
            }

            let fastmc_bin = std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "fastmc".to_string());

            let dir_str = dir.to_string_lossy();
            let tmux_cmd = format!(
                "tmux new-session -d -s {} -c '{}' {} supervisor '{}'",
                sess,
                dir_str.replace("'", "'\\''"),
                fastmc_bin,
                dir_str.replace("'", "'\\''")
            );
            let _ = Command::new("sh")
                .args(&["-c", &tmux_cmd])
                .status();

            success(&format!("Started: {}", name));
            started += 1;
        }
    }
    if started == 0 {
        info("No stopped initialized servers found.");
    }
    println!();
    prompt_line("Press Enter to continue...");
    Ok(())
}


pub fn stop_all_servers(servers: &[PathBuf]) -> Result<()> {
    print_banner();
    title("Stop all servers");
    sep();
    println!();

    let mut stopped = 0usize;
    for dir in servers {
        if server_running(dir) {
            let name = server_name(dir);
            let sess = session_name(dir);
            fs::write(sm_file(dir, "stopping"), "")?;

            let _ = Command::new("tmux")
                .args(&["send-keys", "-t", &sess, "stop", "Enter"])
                .status();

            success(&format!("Stop sent to: {}", name));
            stopped += 1;
        }
    }
    if stopped == 0 {
        info("No servers are currently running.");
    }
    sep();
    prompt_line("Press Enter to continue...");
    Ok(())
}


pub fn broadcast_all(servers: &[PathBuf]) -> Result<()> {
    print_banner();
    title("Broadcast to all running servers");
    sep();
    println!();

    let running: Vec<_> = servers.iter()
        .filter(|d| server_running(d))
        .collect();

    if running.is_empty() {
        warn("No servers are currently running.");
        sep();
        prompt_line("Press Enter to continue...");
        return Ok(());
    }

    let running_names: Vec<_> = running.iter().map(|d| server_name(d)).collect();
    info(&format!("Running servers: {}", running_names.join(" ")));
    println!();

    let cmd = prompt_line("Command to broadcast");

    for dir in &running {
        let name = server_name(dir);
        if rcon_available(dir) {
            let _ = rcon_send(dir, &cmd);
            success(&format!("Sent to {}", name));
        } else {
            let sess = session_name(dir);
            let _ = Command::new("tmux")
                .args(&["send-keys", "-t", &sess, &cmd, "Enter"])
                .status();
            success(&format!("Sent to {} (tmux)", name));
        }
    }

    sep();
    prompt_line("Press Enter to continue...");
    Ok(())
}
fn get_server_pid(dir: &Path) -> Option<i32> {
    let dir_real = dir.canonicalize().ok()?;
    let dir_str = dir_real.to_string_lossy().to_string();
    if let Ok(out) = Command::new("pgrep").args(&["-f", "java.*server.jar"]).output() {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(pid) = line.parse::<i32>() {
                    let comm = fs::read_to_string(format!("/proc/{}/comm", pid)).unwrap_or_default();
                    if comm.trim() != "java" {
                        continue;
                    }
                    if let Ok(cmdline) = fs::read_to_string(format!("/proc/{}/cmdline", pid)) {
                        let normalized = cmdline.replace('\0', " ");
                        if normalized.contains(&dir_str) || normalized.contains("-jar server.jar") {
                            return Some(pid);
                        }
                    }
                }
            }
        }
    }
    None
}

fn resource_monitor(dir: &Path) -> Result<()> {
    let name = server_name(dir);

    print_banner();
    title(&format!("Resource monitor — {}", name));
    sep();
    println!();

    info(&format!("Monitoring {} — press Ctrl+D to exit", name.bold()));
    println!();
    println!("  {:<10}  {:<22}  {}", "CPU %", "RAM RSS / heap", "Heap %");
    sep();

    let num_cpus = std::thread::available_parallelism().map(|n| n.get() as f64).unwrap_or(1.0);
    let mut prev_cpu_total: Option<u64> = None;
    let mut prev_sample_time: Option<std::time::Instant> = None;

    let stdin_fd = io::stdin().as_raw_fd();
    let orig_flags = unsafe { libc::fcntl(stdin_fd, libc::F_GETFL) };
    unsafe { libc::fcntl(stdin_fd, libc::F_SETFL, orig_flags | libc::O_NONBLOCK) };

    loop {
        let mut buf = [0u8; 1];
        match io::stdin().lock().read(&mut buf) {
            Ok(0) => break,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            _ => {}
        }

        if !server_running(dir) {
            warn("Server stopped.");
            break;
        }

        if let Some(pid) = get_server_pid(dir) {
            let now = std::time::Instant::now();

            let cpu_pct = if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) {
                let parts: Vec<&str> = stat.split_whitespace().collect();
                if let (Some(utime), Some(stime)) = (parts.get(13), parts.get(14)) {
                    let total = utime.parse::<u64>().unwrap_or(0) + stime.parse::<u64>().unwrap_or(0);
                    if let (Some(prev_total), Some(prev_time)) = (prev_cpu_total, prev_sample_time) {
                        let delta_total = total.saturating_sub(prev_total);
                        let delta_time = now.duration_since(prev_time).as_secs_f64();
                        if delta_time > 0.0 {
                            let usage = unsafe {
                            let clk_tck = libc::sysconf(libc::_SC_CLK_TCK) as f64;
                            (delta_total as f64 / clk_tck / delta_time) * 100.0 / num_cpus
                        };
                        usage
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };
            prev_cpu_total = if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) {
                let parts: Vec<&str> = stat.split_whitespace().collect();
                let utime = parts.get(13).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let stime = parts.get(14).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                Some(utime + stime)
            } else {
                None
            };
            prev_sample_time = Some(now);

            let mem_kb = Command::new("ps")
                .args(&["-p", &pid.to_string(), "-o", "rss="])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().unwrap_or(0))
                .unwrap_or(0);

            let rss_mb = mem_kb / 1024;
            let heap_mib = get_ram_mib(dir).unwrap_or(1024);
            let heap_gib = heap_mib as f64 / 1024.0;
            let heap_pct = if heap_mib > 0 { (rss_mb * 100 / heap_mib) as u64 } else { 0 };

            print_banner();
            title(&format!("Resource monitor — {}", name));
            sep();
            println!();
            info(&format!("Monitoring {} — press Ctrl+D to exit", name.bold()));
            println!();
            println!("  {:<10}  {:<22}  {}", "CPU %", "RAM RSS / heap", "Heap %");
            sep();
            println!("  {:<10}  {:<22}  {}%", format!("{:.2}%", cpu_pct), format!("{}MB / {:.1}G", rss_mb, heap_gib), heap_pct);
            sep();
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    unsafe { libc::fcntl(stdin_fd, libc::F_SETFL, orig_flags) };
    println!();
    sep();
    Ok(())
}
fn manage_whitelist(dir: &Path) -> Result<()> {
    let wl_file = sm_file(dir, "whitelist");
    let name = server_name(dir);

    loop {
        print_banner();
        title(&format!("Whitelist — {}", name));
        sep();
        println!();

        let entries: Vec<String> = if wl_file.exists() {
            fs::read_to_string(&wl_file)?
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        if entries.is_empty() {
            println!("  {}", "(no players whitelisted)".dimmed());
        } else {
            for e in &entries {
                let pname = e.split_whitespace().next().unwrap_or("");
                println!("  {}", pname);
            }
        }

        println!();
        sep();
        println!();
        let wl_enabled = whitelist_enabled(dir);
        let toggle_label = if wl_enabled { "Disable whitelist" } else { "Enable whitelist" };
        println!("  {}. {}", "1".cyan().bold(), toggle_label.dimmed());
        println!("  {}. Add player", "2".cyan().bold());
        println!("  {}. Remove player", "3".cyan().bold());
        println!("  {}. Clear whitelist", "4".cyan().bold());
        println!("  {}. Back", "5".cyan().bold());

        let choice = prompt_usize("");

        match choice {
            1 => {
                set_whitelist(dir, !wl_enabled)?;
                success(&format!("Whitelist {}.", if wl_enabled { "disabled" } else { "enabled" }));
            }
            2 => {
                println!();
                let pname = prompt_line("Player name to add");

                if !pname.is_empty() {
                    info(&format!("Looking up UUID for {}...", pname));
                    let puuid = lookup_player_uuid(&pname);
                    let puuid = match puuid {
                        Some(u) => {
                            success(&format!("Found UUID: {}", u.dimmed()));
                            u
                        }
                        None => {
                            warn("Could not look up UUID — adding without UUID (server may reject on join).");
                            "00000000-0000-0000-0000-000000000000".to_string()
                        }
                    };
                    if wl_file.exists() {
                        let mut current = fs::read_to_string(&wl_file)?;
                        current.push_str(&format!("{} {}\n", pname, puuid));
                        fs::write(&wl_file, current)?;
                    } else {
                        fs::write(&wl_file, format!("{} {}\n", pname, puuid))?;
                    }
                    rebuild_whitelist_json(dir)?;
                    success(&format!("Added {}.", pname));
                }
            }
            3 => {
                if entries.is_empty() {
                    warn("No players to remove.");
                } else {
                    println!();
                    for (i, e) in entries.iter().enumerate() {
                        let pname = e.split_whitespace().next().unwrap_or("");
                        println!("  {}. {}", (i+1).to_string().cyan().bold(), pname);
                    }
                    println!();
                    let idx = prompt_usize("Player number to remove");
                    if idx > 0 && idx <= entries.len() {
                        let rem_name = entries[idx - 1].split_whitespace().next().unwrap_or("");
                        let rem_content = fs::read_to_string(&wl_file)
                            .unwrap_or_default()
                            .lines()
                            .filter(|l| l.split_whitespace().next().map(|n| n != rem_name).unwrap_or(true))
                            .collect::<Vec<_>>()
                            .join("\n");
                        fs::write(&wl_file, rem_content)?;
                        rebuild_whitelist_json(dir)?;
                        success(&format!("Removed {}.", rem_name));
                    }
                }
            }
            4 => {
                if entries.is_empty() {
                    warn("Whitelist is already empty.");
                } else {
                    println!();
                    let confirm = prompt_line("Clear whitelist? [No/yes]:");
                    if confirm.trim().eq_ignore_ascii_case("yes") {
                        fs::write(&wl_file, "")?;
                        rebuild_whitelist_json(dir)?;
                        success("Whitelist cleared.");
                    } else {
                        warn("Cleared cancelled.");
                    }
                }
            }
            5 => return Ok(()),
            _ => warn("Invalid choice."),
        }

        println!();
        sep();
        prompt_line("Press Enter to continue...");
    }
}

fn rebuild_whitelist_json(dir: &Path) -> Result<()> {
    let wl_file = sm_file(dir, "whitelist");
    let mut json = "[".to_string();
    let mut first = true;

    if wl_file.exists() {
        for line in fs::read_to_string(&wl_file)?.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if !first { json.push(','); }
                json.push_str(&format!(r#"{{"uuid":"{}","name":"{}"}}"#, parts[1], parts[0]));
                first = false;
            }
        }
    }
    json.push(']');
    fs::write(dir.join("whitelist.json"), &json)?;
    Ok(())
}

