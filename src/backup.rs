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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{rcon_password, rcon_port};
use crate::rcon::rcon_send;
use crate::{backup_base, print_banner, prompt_usize, sep, server_name, server_running, title, info, prompt_line};

fn display_path(path: &Path) -> String {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    if let Ok(stripped) = path.strip_prefix(home) {
        if stripped.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}

pub fn backup_server(dir: &Path) -> Result<()> {
    let name = server_name(dir);
    let backup_dir = backup_base().join(&name);
    fs::create_dir_all(&backup_dir)?;

    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%s").to_string();

    print_banner();
    title(&format!("Backup — {}", name));
    sep();
    println!();
    println!("  {} {}", "Server:".dimmed(), display_path(dir));
    println!("  {} {}", "Backups:".dimmed(), display_path(&backup_dir));
    println!();
    sep();
    println!("  {}. ZIP", "1".cyan().bold());
    println!("  {}. TAR", "2".cyan().bold());
    println!("  {}. tar.gz", "3".cyan().bold());
    println!("  {}. tar.xz", "4".cyan().bold());
    println!("  {}. Custom command", "5".cyan().bold());
    println!("  {}. Quit", "6".cyan().bold());
    println!();

    let choice = prompt_usize("Choose backup format:");

    let rcon_used = server_running(dir) && !rcon_password(dir).is_empty() && !rcon_port(dir).is_empty();
    if rcon_used {
        info("Server is live — pausing autosave for clean backup...");
        let _ = rcon_send(dir, "save-all");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let _ = rcon_send(dir, "save-off");
    }

    let archive = backup_dir.join(format!("{}_{}", name, timestamp));

    match choice {
        1 => {
            let archive_path = archive.with_extension("zip");
            info("Creating ZIP backup...");
            let output = Command::new("zip")
                .args(&["-r", archive_path.to_str().unwrap(), ".", "-x", "backups/*", "logs/*"])
                .current_dir(dir)
                .output();
            if let Ok(out) = output {
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if !out.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        2 => {
            let archive_path = archive.with_extension("tar");
            info("Creating TAR backup...");
            let output = Command::new("tar")
                .args(&["-cf", archive_path.to_str().unwrap(), "--exclude=backups", "--exclude=logs", "."])
                .current_dir(dir)
                .output();
            if let Ok(out) = output {
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if !out.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        3 => {
            let archive_path = archive.with_extension("tar.gz");
            info("Creating tar.gz backup...");
            let output = Command::new("tar")
                .args(&["-czf", archive_path.to_str().unwrap(), "--exclude=backups", "--exclude=logs", "."])
                .current_dir(dir)
                .output();
            if let Ok(out) = output {
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if !out.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        4 => {
            let archive_path = archive.with_extension("tar.xz");
            info("Creating tar.xz backup...");
            let output = Command::new("tar")
                .args(&["-cJf", archive_path.to_str().unwrap(), "--exclude=backups", "--exclude=logs", "."])
                .current_dir(dir)
                .output();
            if let Ok(out) = output {
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if !out.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        5 => {
            println!();
            println!("  {}", "Enter your custom backup command.".dimmed());
            println!("  {}", "Use {{src}} for server path and {{dest}} for backup path.".dimmed());
            println!();
            let custom_cmd = prompt_line("Command:");
            let src_display = display_path(dir);
            let dest_display = display_path(&backup_dir);
            let cmd = custom_cmd
                .replace("{src}", &src_display)
                .replace("{dest}", &dest_display);
            info(&format!("Running: {}", cmd));
            println!();
            let output = Command::new("sh")
                .args(&["-c", &cmd])
                .current_dir(dir)
                .output();
            if let Ok(out) = output {
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if !out.stderr.is_empty() {
                    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                }
            }
        }
        _ => {}
    }

    if rcon_used {
        let _ = rcon_send(dir, "save-on");
    }

    sep();
    prompt_line("Press Enter to continue...");
    Ok(())
}

