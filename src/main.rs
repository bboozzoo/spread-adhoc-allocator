// SPDX-FileCopyrightText: 2024 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::fs;

use anyhow::Context;
use clap::{Parser, Subcommand};
use log;
use simple_logger;

mod config;
mod lxd;

use anyhow::{anyhow, Result};

const BUILD_GIT_VERSION: &str = env!["BUILD_GIT_VERSION"];
const VERSION: &str = env!["CARGO_PKG_VERSION"];

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Allocate a spread system.
    Allocate {
        /// System name
        name: String,
        /// User name for remote access.
        user: String,
        /// Password for remote access.
        password: String,
    },
    /// Deallocate a system.
    Deallocate {
        /// Addess, in form of <ip>:<ssh-port>, of a node to deallocate.
        addr_port: String,
    },
    /// Cleanup all allocated systems.
    Cleanup,
    /// Show version information.
    Version,
}

fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Trace).unwrap();

    let cli = Cli::parse();

    let conf_name = lxd::config_file_name();

    match cli.command {
        Some(Command::Allocate {
            name: sysname,
            user,
            password,
        }) => {
            let cfg_path = config::locate(lxd::config_file_name())
                .with_context(|| format!("cannot find config file {}", conf_name))?;

            log::debug!("loading config from {}", cfg_path.to_string_lossy());

            let cfg = fs::File::open(cfg_path).context("cannot open config file")?;

            let alloc = lxd::allocator_with_config(cfg).context("cannot set up allocator")?;
            let res = alloc
                .allocate(
                    &sysname,
                    lxd::RemoteUserAccessConfig {
                        user: &user,
                        password: &password,
                    },
                )
                .context("cannot allocate");
            match res {
                Ok(instance) => {
                    println!("{}:{}", instance.addr, instance.ssh_port);
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
        Some(Command::Deallocate { addr_port }) => {
            let sp: Vec<&str> = addr_port.split(":").collect();
            if sp.len() != 2 {
                return Err(anyhow!("invalid address, expected <addr>:<port>"));
            }

            let addr = sp.get(0).unwrap();

            lxd::LxdAllocator::new()
                .deallocate_by_addr(&addr)
                .with_context(|| format!("cannot deallocate system with address {}", addr))
        }
        Some(Command::Cleanup) => lxd::LxdAllocator::new()
            .deallocate_all()
            .context("cannot cleanup all nodes"),
        Some(Command::Version) => {
            println!("{} (git {})", VERSION, BUILD_GIT_VERSION);
            Ok(())
        }
        None => Err(anyhow!("no command provided, see --help")),
    }
}
