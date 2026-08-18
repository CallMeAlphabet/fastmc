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

use reqwest::blocking::Client;

pub fn download_url(stype: &str, version: &str) -> String {
    match stype {
        "leafmc" => "https://www.leafmc.one/download".to_string(),
        "paper" => "https://fill-ui.papermc.io/projects/paper".to_string(),
        "purpur" => format!("https://purpurmc.org/download/purpur/{}", version),
        "spigot" => "https://www.spigotmc.org/wiki/buildtools/".to_string(),
        "fabric" => "https://fabricmc.net/use/server/".to_string(),
        "vanilla" => format!("https://www.minecraft.net/en-us/article/minecraft-java-edition-{}", version.replace('.', "-")),
        _ => String::new(),
    }
}

pub fn get_public_ip() -> String {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok();

    if let Some(client) = client {
        if let Ok(resp) = client.get("https://api.ipify.org").send() {
            if let Ok(ip) = resp.text() {
                let ip = ip.trim().to_string();
                if !ip.is_empty() {
                    return ip;
                }
            }
        }
    }
    "unknown".to_string()
}

pub fn lookup_player_uuid(name: &str) -> Option<String> {
    let url = format!("https://api.mojang.com/users/profiles/minecraft/{}", name);
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(&url).send().ok()?;
    let text = resp.text().ok()?;

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
            if id.len() == 32 {
                return Some(format!(
                    "{}-{}-{}-{}-{}",
                    &id[0..8],
                    &id[8..12],
                    &id[12..16],
                    &id[16..20],
                    &id[20..32]
                ));
            }
        }
    }
    None
}

pub fn open_url(url: &str) -> bool {
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .spawn();

    true
}
