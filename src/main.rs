use std::env;

mod config;
mod lxd;

fn main() {
    for argument in env::args() {
        println!("{argument}");
    }

    let mut args = env::args().skip(1);
    let action = args.next().expect("no action");
    //let cfg = config::find().expect("config file not found");

    match action.as_ref() {
        "allocate" => {
            let sysname = args.next().expect("no system name");
            if let Err(err) = lxd::allocate(&sysname) {
                eprintln!("cannot allocate: {}", err)
            }
        }
        _ => {
            panic!("unknown action {}", action)
        }
    }
}
