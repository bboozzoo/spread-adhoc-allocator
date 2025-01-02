use std::env;
use std::fs;

use log;
use simple_logger;

mod config;
mod lxd;

fn main() {
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    let mut args = env::args().skip(1);
    let action = args.next().expect("no action");
    let cfg_path = config::locate(lxd::config_file_name()).expect("config file not found");
    let cfg = fs::File::open(cfg_path).expect("cannot open config file");
    let alloc = lxd::allocator_with_config(cfg).expect("cannot create allocator");

    match action.as_ref() {
        "allocate" => {
            let sysname = args.next().expect("no system name");
            match alloc.allocate(&sysname) {
                Ok(instance) => {
                    println!("{}:{}", instance.addr, instance.ssh_port);
                }
                Err(err) => {
                    log::error!("cannot allocate: {}", err)
                }
            }
        }
        "deallocate" => {
            let addr = args.next().expect("no address");
            if let Err(err) = alloc.deallocate_by_addr(&addr) {
                log::error!("cannot deallocate: {}", err)
            }
        }
        "cleanup" => {
            if let Err(err) = alloc.deallocate_all() {
                log::error!("cannot deallocate all systems: {}", err)
            }
        }
        _ => {
            log::error!("unknown action {}", action)
        }
    }
}
