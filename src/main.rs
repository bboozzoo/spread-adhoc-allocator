mod lxd;

fn main() {
    let res = lxd::run_lxc(&["foo"]);
    match res {
        Err(err) => eprintln!("cannot run lxc command: {}", err),
        Ok(out) => eprintln!("lxc output:\n{}", String::from_utf8_lossy(&out)),
    }
}
