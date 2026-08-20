# fastmc
fastmc — create a Minecraft server in under a minute

## Table of Contents
- [Backstory](#backstory)
- [Features](#features)
- [Dependencies](#dependencies)
- [Build and Install](#build-and-install)
- [Uninstall](#uninstall)
- [Directory Structure](#directory-structure)


## Backstory

A friend once bet me an 8GB DDR4 RAM, 20-core Minecraft server if I could set up a LeafMC server in under 5 minutes. I said "no" about ten times. With RAM prices being what they are (or were), the odds of him actually following through seemed close to zero. But eventually, I took the challenge, opened my terminal, fired up a server and got it done (then deleted the server right after).

That experience got me thinking: why not automate this? So I built a tool to create, backup, delete, and manage Minecraft servers, and decided to make it public for anyone who needs it. Or lands in a situation like me. 


## Features

- Multi-software support for LeafMC, Paper, Purpur, Spigot, Fabric, and Vanilla
- Named servers where you give each server a name and the version plus software type are saved
- Crash detection and auto-restart. Servers restart automatically on crash. After 3 crashes in 10 minutes the supervisor gives up and logs what happened
- Crash logs written to `~/.local/share/fastmc/LOGS/YYYY-MM-DD.log` with timestamps and exit codes
- Live resource monitor showing CPU percent and RAM used versus allocated
- Backup and restore with configurable retention (default keeps the last 5)
- Scheduled backups so you can set up automatic backups via cron right from the menu
- Native RCON integration to send commands to running servers without leaving the manager
- Multi-server orchestration to start all or stop all servers from the main menu
- tmux-based so servers survive terminal disconnects and you can attach anytime


## Dependencies

You need these packages installed:

On Arch:
```bash
sudo pacman -S tmux curl openssl
```
You also need Java and the Rust toolchain.

## Build and Install

```bash
cargo build --release
sudo cp target/release/fastmc /usr/bin/fastmc
```

Run the manager:
```bash
fastmc
```

## Uninstall

**This will also delete your Minecraft servers and their data**
```bash
sudo rm /usr/bin/fastmc
rm -rf ~/.local/share/fastmc
```

## Directory Structure

```
~/.local/share/fastmc/
├── backups/
│   └── <server-name>/     ← .tar.gz backups per server
├── LOGS/
│   └── YYYY-MM-DD.log     ← crash logs
└── <server-name>/
    ├── server.jar
    ├── start.sh
    ├── server.properties
    ├── eula.txt
    ├── .mcserver.conf      ← RCON credentials
    └── fastmc/
        ├── type            ← e.g. "paper"
        ├── version         ← e.g. "1.21.11"
        ├── backup_keep     ← retention count
        └── stopping        ← flag file for intentional stops
```
