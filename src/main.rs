use std::env;

use log;
use simple_logger;

mod config;
mod lxd;

fn main() {
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    for argument in env::args() {
        log::info!("{argument}");
    }

    let mut args = env::args().skip(1);
    let action = args.next().expect("no action");
    //let cfg = config::find().expect("config file not found");

    match action.as_ref() {
        "allocate" => {
            let sysname = args.next().expect("no system name");
            if let Err(err) = lxd::allocator().allocate(&sysname) {
                log::error!("cannot allocate: {}", err)
            }
        }
        _ => {
            log::error!("unknown action {}", action)
        }
    }
}
