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
use std::fs;
use std::path::{Path, PathBuf};

 pub fn sm_dir(dir: &Path) -> PathBuf {
    dir.join("fastmc")
}

pub fn sm_file(dir: &Path, key: &str) -> PathBuf {
    sm_dir(dir).join(key)
}

pub fn read_meta(dir: &Path, key: &str) -> String {
    let f = sm_file(dir, key);
    if f.exists() {
        fs::read_to_string(&f).unwrap_or_default().trim().to_string()
    } else {
        String::new()
    }
}

pub fn write_meta(dir: &Path, key: &str, value: &str) {
    let dir_path = sm_dir(dir);
    fs::create_dir_all(&dir_path).ok();
    let f = sm_file(dir, key);
    fs::write(&f, format!("{}\n", value)).ok();
}

pub fn server_name(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().to_string()
}

pub fn server_type(dir: &Path) -> String {
    read_meta(dir, "type")
}

pub fn server_version(dir: &Path) -> String {
    read_meta(dir, "version")
}

pub fn server_notes(dir: &Path) -> String {
    read_meta(dir, "notes")
}

pub fn server_port(dir: &Path) -> String {
    read_meta(dir, "port")
}

pub fn whitelist_enabled(dir: &Path) -> bool {
    let props = dir.join("server.properties");
    if let Ok(content) = fs::read_to_string(&props) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("white-list=") {
                return line.split('=').nth(1).map(|v| v == "true").unwrap_or(false);
            }
        }
    }
    false
}

pub fn set_whitelist(dir: &Path, enabled: bool) -> Result<()> {
    let props = dir.join("server.properties");
    let content = fs::read_to_string(&props)?;
    let new_content = if content.contains("white-list=") {
        content.lines()
            .map(|l| if l.trim().starts_with("white-list=") { format!("white-list={}", enabled) } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("{}\nwhite-list={}", content, enabled)
    };
    fs::write(&props, new_content)?;
    Ok(())
}

pub fn conf_path(dir: &Path) -> PathBuf {
    dir.join(".mcserver.conf")
}

pub fn read_conf(dir: &Path, key: &str) -> String {
    let conf = conf_path(dir);
    if conf.exists() {
        if let Ok(content) = fs::read_to_string(&conf) {
            for line in content.lines() {
                if line.starts_with(&format!("{}=", key)) {
                    return line.trim_start_matches(&format!("{}=", key)).to_string();
                }
            }
        }
    }
    String::new()
}

pub fn write_conf(dir: &Path, key: &str, value: &str) {
    let conf = conf_path(dir);
    let existing = fs::read_to_string(&conf).unwrap_or_default();
    let new_lines: Vec<String> = existing
        .lines()
        .filter(|line| !line.starts_with(&format!("{}=", key)))
        .map(|s| s.to_string())
        .chain(std::iter::once(format!("{}={}", key, value)))
        .collect();
    fs::write(&conf, format!("{}\n", new_lines.join("\n"))).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&conf, std::fs::Permissions::from_mode(0o600)).ok();
    }
}

pub fn rcon_password(dir: &Path) -> String {
    read_conf(dir, "rcon_password")
}

pub fn rcon_port(dir: &Path) -> String {
    read_conf(dir, "rcon_port")
}
