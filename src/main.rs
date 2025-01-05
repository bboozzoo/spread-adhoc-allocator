use std::env;
use std::fs;

use anyhow::Context;
use log;
use simple_logger;

mod config;
mod lxd;

use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    let mut args = env::args().skip(1);
    let action = args.next().expect("no action");
    let cfg_path = config::locate(lxd::config_file_name()).expect("config file not found");
    let cfg = fs::File::open(cfg_path).expect("cannot open config file");
    let alloc = lxd::allocator_with_config(cfg).expect("cannot create allocator");

    match action.as_ref() {
        "allocate" => {
            let sysname = args.next().expect("no system name");
            let user = args.next().expect("no user name");
            let password = args.next().expect("no password");
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
            let addr = args.next().expect("no address");
            alloc
                .deallocate_by_addr(&addr)
                .with_context(|| format!("cannot deallocate system with address {}", addr))
        }
        "cleanup" => alloc.deallocate_all().context("cannot cleanup all nodes"),
        _ => Err(anyhow!("unknown action {}", action)),
    }
}
