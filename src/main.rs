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
use colored::*;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

mod backup;
mod config;
mod download;
mod rcon;
mod server;

use download::*;
use server::*;

fn data_dir() -> PathBuf {
    dirs::home_dir().unwrap().join(".local/share/fastmc")
}

fn servers_dir() -> PathBuf {
    data_dir()
}

fn backup_base() -> PathBuf {
    data_dir().join("backups")
}

fn log_base() -> PathBuf {
    data_dir().join("LOGS")
}

const TMUX_PREFIX: &str = "mcserver";

fn sep() {
    println!("{}", "─".repeat(60).cyan());
}

fn title(msg: &str) {
    println!("  {}", msg.bold());
}

fn subtitle(msg: &str) {
    println!("  {}", msg.dimmed());
}

fn info(msg: &str) {
    println!("{} {}", "[*]".cyan().bold(), msg);
}

fn success(msg: &str) {
    println!("{} {}", "[✓]".green().bold(), msg);
}

fn warn(msg: &str) {
    println!("{} {}", "[!]".yellow().bold(), msg);
}

fn print_banner() {
    print!("\x1b[2J\x1b[0;0H");
    sep();
    println!("  {}fastmc{}", " ".bold().green(), " ".normal());
    println!("   {}", data_dir().display().to_string().dimmed());
    sep();
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let first = s.chars().next().unwrap();
        let last = s.chars().last().unwrap();
        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

static ANSI_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap());

fn strip_ansi(s: &str) -> String {
    ANSI_RE.replace_all(s, "").to_string()
}

fn prompt_line(prompt: &str) -> String {
    println!();
    print!("> {} ", prompt);
    io::stdout().flush().ok();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_ok() {
        strip_ansi(&line.trim()).to_string()
    } else {
        String::new()
    }
}

pub fn edit_notes_interactive(initial: &str) -> String {
    let mut notes = initial.to_string();

    print!("{}", "  (Press Ctrl+D to save and exit)\n\n".dimmed());
    print!("{}", notes);
    io::stdout().flush().ok();

    let stdin_fd = libc::STDIN_FILENO;
    let mut orig_termios: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        libc::tcgetattr(stdin_fd, &mut orig_termios);
        let mut raw = orig_termios;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(stdin_fd, libc::TCSADRAIN, &raw);
    }

    let mut buf = [0u8; 1];
    loop {
        if io::stdin().lock().read(&mut buf).unwrap_or(0) == 0 {
            break;
        }
        match buf[0] {
            4 => break,
            8 | 127 => {
                if !notes.is_empty() {
                    notes.pop();
                    print!("\x08 \x08");
                    io::stdout().flush().ok();
                }
            }
            c if c >= 32 && c < 127 => {
                notes.push(c as char);
                print!("{}", c as char);
                io::stdout().flush().ok();
            }
            _ => {}
        }
    }

    unsafe {
        libc::tcsetattr(stdin_fd, libc::TCSADRAIN, &orig_termios);
    }

    println!();
    notes
}

fn prompt_usize(prompt: &str) -> usize {
    prompt_line(prompt).parse().unwrap_or(0)
}

fn prompt_menu(prompt: &str, max: usize) -> usize {
    loop {
        let v = prompt_usize(prompt);
        if v == 0 || v > max {
            warn("Pick an option.");
            continue;
        }
        return v;
    }
}

fn prompt_yn(prompt: &str, default_yes: bool) -> bool {
    let ans = prompt_line(prompt);
    let ans = ans.trim().to_lowercase();
    if ans.is_empty() {
        return default_yes;
    }
    if default_yes {
        ans != "n"
    } else {
        ans == "y"
    }
}

 fn sm_dir(dir: &Path) -> PathBuf {
    dir.join("fastmc")
}

fn sm_file(dir: &Path, key: &str) -> PathBuf {
    sm_dir(dir).join(key)
}

fn read_meta(dir: &Path, key: &str) -> String {
    let f = sm_file(dir, key);
    if f.exists() {
        fs::read_to_string(&f).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    }
}

fn write_meta(dir: &Path, key: &str, value: &str) {
    let dir_path = sm_dir(dir);
    fs::create_dir_all(&dir_path).ok();
    let f = sm_file(dir, key);
    fs::write(&f, format!("{}\n", value)).ok();
}

fn server_name(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().to_string()
}

fn server_type(dir: &Path) -> String {
    read_meta(dir, "type")
}

fn server_version(dir: &Path) -> String {
    read_meta(dir, "version")
}










fn check_deps() -> Vec<String> {
    let mut missing = Vec::new();
    for cmd in ["tmux", "mcrcon", "curl"] {
        if !Command::new("which").arg(cmd).output().map(|o| o.status.success()).unwrap_or(false) {
            missing.push(cmd.to_string());
        }
    }
    missing
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "supervisor" {
        if args.len() >= 3 {
            let dir = PathBuf::from(&args[2]);
            if dir.exists() {
                return server::run_supervisor(&dir);
            }
        }
        eprintln!("Usage: fastmc supervisor <server-dir>");
        std::process::exit(1);
    }

    let missing = check_deps();
    if !missing.is_empty() {
        warn(&format!("Missing required dependencies: {}", missing.join(", ")));
        println!();
        prompt_line("Press Enter to continue...");
    }

    fs::create_dir_all(servers_dir())?;
    fs::create_dir_all(backup_base())?;
    fs::create_dir_all(log_base())?;

    for entry in fs::read_dir(servers_dir())? {
        let entry = entry?;
        let old_meta = entry.path().join("ServerManager");
        let new_meta = entry.path().join("fastmc");
        if old_meta.exists() && !new_meta.exists() {
            let _ = fs::rename(&old_meta, &new_meta);
        }
    }

    loop {
        print_banner();
        let servers = scan_servers();

        if servers.is_empty() {
            title("No servers found");
            sep();
            println!();
            println!("  {}. Add a new server", "1".cyan().bold());
            println!("  {}. Quit", "2".cyan().bold());

            let choice = prompt_usize("");

            match choice {
                1 => add_new_server()?,
                2 => {
                    info("Bye!");
                    break;
                }
                _ => warn("Pick an option."),
            }
            continue;
        }

        title("Your servers");
        sep();

        for (i, dir) in servers.iter().enumerate() {
            let name = server_name(dir);
            let stype = server_type(dir);
            let version = server_version(dir);
            let state = server_state(dir);
            let ram = get_ram(dir);

            let dot = if server_running(dir) { "●".green().bold().to_string() } else { "○".dimmed().to_string() };
            let badge = match state {
                "fresh" => "needs setup".yellow().to_string(),
                "initialized" => format!("{} · {} {} · RAM: {}G", "ready".green(), stype, version, ram.bold()),
                "broken" => "broken".red().to_string(),
                _ => "?".dimmed().to_string(),
            };

            println!("  {}. {}  {}  {}", (i + 1).to_string().cyan().bold(), dot, name.bold(), format!("| {}", badge).dimmed());
        }

        sep();

        println!();

        let n = servers.len();
        println!("  {}. Start all servers", (n + 1).to_string().cyan().bold());
        println!("  {}. Stop all servers", (n + 2).to_string().cyan().bold());
        println!("  {}. Broadcast command to all", (n + 3).to_string().cyan().bold());
        println!("  {}. View crash logs", (n + 4).to_string().cyan().bold());
        println!("  {}. Add a new server", (n + 5).to_string().cyan().bold());
        println!("  {}. Quit", (n + 6).to_string().cyan().bold());

        let choice = prompt_usize("");

        match choice {
            c if c == n + 6 => {
                info("Bye!");
                break;
            }
            c if c == n + 5 => add_new_server()?,
            c if c == n + 1 => start_all_servers(&servers)?,
            c if c == n + 2 => stop_all_servers(&servers)?,
            c if c == n + 3 => broadcast_all(&servers)?,
            c if c == n + 4 => view_crash_logs()?,
            c if c >= 1 && c <= n => manage_server(&servers[c - 1])?,
            _ => warn("Pick an option."),
        }
    }

    Ok(())
}

fn add_new_server() -> Result<()> {
    print_banner();
    title("Add a new server");
    sep();

    let srv_name: String;
    loop {
        let raw: String = prompt_line("Server name (e.g. survival, creative, lobby):");
        let candidate = raw.replace(' ', "-");

        if candidate.is_empty() {
            warn("Type in a name");
            continue;
        }

        let target = servers_dir().join(&candidate);
        if target.exists() {
            let ans = prompt_line("Server with that name already exists, overwrite? [No/yes]:");
            if ans.trim().to_lowercase() != "yes" {
                warn("Type in a different name");
                continue;
            }
            let ans2 = prompt_line("Overwrite? [No/yes]:");
            if ans2.trim().to_lowercase() == "yes" {
                info("Overwriting...");
                let _ = fs::remove_dir_all(&target);
            } else {
                warn("Type in a different name");
                continue;
            }
        }

        srv_name = candidate;
        break;
    }

    let srv_version: String;
    loop {
        let v = prompt_line("Minecraft version (e.g. 1.21.4):");
        if v.trim().is_empty() {
            warn("Type in the server version");
            continue;
        }
        srv_version = v;
        break;
    }

    let srv_types = ["leafmc", "paper", "purpur", "spigot", "fabric", "vanilla"];
    let srv_labels = [
        "LeafMC - optimised fork of Paper",
        "Paper - most popular high-performance software",
        "Purpur - Paper fork with extra configurability",
        "Spigot - classic, requires BuildTools",
        "Fabric - lightweight mod loader",
        "Vanilla - official Mojang server",
    ];

    print_banner();
    title("Add a new server");
    sep();
    println!();
    title("Choose server software");
    for (i, label) in srv_labels.iter().enumerate() {
        println!("  {}. {}", (i + 1).to_string().cyan().bold(), label);
    }
    let srv_type = prompt_menu("", srv_types.len()) - 1;

    let dest_dir = servers_dir().join(&srv_name);
    fs::create_dir_all(&dest_dir)?;
    fs::create_dir_all(sm_dir(&dest_dir))?;

    let url = download_url(srv_types[srv_type], &srv_version);
    println!();
    println!("  {}Download page:{} {}", " ".bold(), " ".normal(), url.cyan());

    let open_browser = prompt_yn("Open in browser? [Y/n]", true);
    if open_browser {
        let _ = open_url(&url);
    }

    let jar_src: String;
    loop {
        let raw = prompt_line("Full path to the downloaded JAR:");
        let expanded = strip_quotes(&raw);
        let expanded = shellexpand::tilde(&expanded).into_owned();
        let jar_src_path = PathBuf::from(&expanded);
        if jar_src_path.exists() {
            jar_src = expanded;
            break;
        }
        warn(&format!("File not found: {}", expanded));
    }

    let dest_jar = dest_dir.join("server.jar");
    if PathBuf::from(&jar_src) != dest_jar {
        move_file(&jar_src, &dest_jar)?;
    }
    success(&format!("JAR moved to {}/server.jar", dest_dir.join("server.jar").display()));

    write_meta(&dest_dir, "type", srv_types[srv_type]);
    write_meta(&dest_dir, "version", &srv_version);
    write_meta(&dest_dir, "backup_keep", "5");

    run_first_time_setup(&dest_dir)?;
    Ok(())
}



