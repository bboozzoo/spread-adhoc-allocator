mod config;
mod lxd;

fn main() {
    let cfg = config::find().expect("config file not found");

    if let Err(err) = lxd::allocate() {
        eprintln!("cannot allocate: {}", err)
    }
}
