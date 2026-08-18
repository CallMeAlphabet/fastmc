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
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use crate::config::{rcon_password, rcon_port};

pub fn rcon_send(dir: &Path, cmd: &str) -> Result<()> {
    let pass = rcon_password(dir);
    let port = rcon_port(dir);
    if pass.is_empty() || port.is_empty() {
        return Ok(());
    }

    let addr = format!("127.0.0.1:{}", port);
    if let Ok(mut stream) = TcpStream::connect(addr) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

        let payload = format!("{}\x00{}\x00\x00", pass, cmd);
        let len = (payload.len() as u32).to_le_bytes();
        let req_id = 1u32.to_le_bytes();
        let ptype = 2u32.to_le_bytes();

        let mut packet = Vec::new();
        packet.extend_from_slice(&len);
        packet.extend_from_slice(&req_id);
        packet.extend_from_slice(&ptype);
        packet.extend_from_slice(payload.as_bytes());

        let _ = stream.write_all(&packet);
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
    }

    Ok(())
}

pub fn rcon_available(dir: &Path) -> bool {
    !rcon_password(dir).is_empty() && !rcon_port(dir).is_empty()
}
