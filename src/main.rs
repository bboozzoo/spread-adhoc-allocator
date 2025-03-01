// SPDX-FileCopyrightText: 2024 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::fs::File;
use std::{fs, io};

use anyhow::Context;
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use log;
use simple_logger;

mod allocator;
mod config;
mod lxd;

const BUILD_GIT_VERSION: &str = env!["BUILD_GIT_VERSION"];
const VERSION: &str = env!["CARGO_PKG_VERSION"];

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Backend {
    Lxd,
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_enum, long, short, default_value_t = Backend::Lxd)]
    backend: Backend,

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
    /// Discard a system.
    Discard {
        /// Addess, in form of <ip>:<ssh-port>, of a node to discard.
        addr_port: String,
    },
    /// Discard all allocated systems.
    Cleanup,
    /// Show version information.
    Version,
}

fn mandatory_config(name: &str) -> Result<File> {
    let cfg_path = config::locate(lxd::config_file_name())
        .with_context(|| format!("cannot find config file {}", name))?;

    log::debug!("loading config from {}", cfg_path.to_string_lossy());

    fs::File::open(cfg_path).context("cannot open config file")
}

fn optional_config() -> Result<Option<File>> {
    if let Some(user_conf_path) = config::user_config() {
        match fs::File::open(&user_conf_path) {
            Ok(f) => {
                log::debug!(
                    "found user configuration file {}",
                    user_conf_path.to_string_lossy()
                );
                Ok(Some(f))
            }
            Err(err) => match err.kind() {
                io::ErrorKind::NotFound => {
                    log::debug!(
                        "user configuration file {} not found",
                        user_conf_path.to_string_lossy()
                    );
                    Ok(None)
                }
                _ => return Err(err).context("cannot open user config file"),
            },
        }
    } else {
        Ok(None)
    }
}

fn initialize_backend(
    backend: &Backend,
    command: Option<&Command>,
) -> Result<Box<dyn allocator::NodeAllocator>> {
    match backend {
        Backend::Lxd => {
            let mut builder = lxd::LxdAllocatorBuilder::new();

            if match command {
                // only allocate needs full configuration
                Some(Command::Allocate { .. }) => true,
                _ => false,
            } {
                builder = builder
                    .with_config(mandatory_config(lxd::config_file_name())?)
                    .context("cannot apply configuration")?;
            }

            let b = builder
                .with_optional_user_config(optional_config()?)
                .context("cannot apply user configuration")?
                .build();
            Ok(Box::new(b))
        }
        _ => return Err(anyhow!("backend {:?} is not supported yet", backend)),
    }
}

fn try_main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Trace).unwrap();

    let cli = Cli::parse();

    let mut b = initialize_backend(&cli.backend, cli.command.as_ref())?;

    match cli.command {
        Some(Command::Allocate {
            name: sysname,
            user,
            password,
        }) => {
            let res = b
                .allocate_by_name(
                    &sysname,
                    allocator::RemoteUserAccessConfig {
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
        Some(Command::Discard { addr_port }) => {
            let sp: Vec<&str> = addr_port.split(":").collect();
            if sp.len() != 2 {
                return Err(anyhow!("invalid address, expected <addr>:<port>"));
            }

            let addr = sp.get(0).unwrap();

            b.discard_by_addr(&addr)
                .with_context(|| format!("cannot discard system with address {}", addr))
        }
        Some(Command::Cleanup) => b.discard_all().context("cannot cleanup all nodes"),
        Some(Command::Version) => {
            println!("{} (git {})", VERSION, BUILD_GIT_VERSION);
            Ok(())
        }
        None => Err(anyhow!("no command provided, see --help")),
    }
}

use std::process;

fn main() -> process::ExitCode {
    if let Err(err) = try_main() {
        println!("{:#}", err);
        process::ExitCode::FAILURE
    } else {
        process::ExitCode::SUCCESS
    }
}
