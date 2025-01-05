use std::env;
use std::fs;

use anyhow::Context;
use log;
use simple_logger;

mod config;
mod lxd;

use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Trace).unwrap();

    let conf_name = lxd::config_file_name();

    let mut args = env::args().skip(1);
    let action = args.next().context("no action")?;

    let cfg_path = config::locate(lxd::config_file_name())
        .with_context(|| format!("cannot find config file {}", conf_name))?;

    log::debug!("loading config from {}", cfg_path.to_string_lossy());

    let cfg = fs::File::open(cfg_path).context("cannot open config file")?;

    let alloc = lxd::allocator_with_config(cfg).context("cannot set up allocator")?;

    match action.as_ref() {
        "allocate" => {
            let sysname = args.next().context("no system name")?;
            let user = args.next().context("no user name")?;
            let password = args.next().context("no password")?;
            let res = alloc
                .allocate(
                    &sysname,
                    lxd::UserConfig {
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
        "deallocate" => {
            let addr = args.next().context("no address")?;
            alloc
                .deallocate_by_addr(&addr)
                .with_context(|| format!("cannot deallocate system with address {}", addr))
        }
        "cleanup" => alloc.deallocate_all().context("cannot cleanup all nodes"),
        _ => Err(anyhow!("unknown action {}", action)),
    }
}
