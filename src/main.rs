use std::error::Error;
use std::fmt;
use std::io;
use std::process::Command;

#[derive(Debug, Clone)]
struct LxcCommandError {
    stderr: Vec<u8>,
    exit_code: i32,
}

#[derive(Debug)]
enum LxcError {
    Start(io::Error),
    Execution(LxcCommandError),
}

impl Error for LxcError {}

impl From<io::Error> for LxcError {
    fn from(error: io::Error) -> Self {
        LxcError::Start(error)
    }
}

impl fmt::Display for LxcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LxcError::Execution(eerr) => {
                write!(
                    f,
                    "lxc command exited with status {}, stderr:\n{}",
                    eerr.exit_code,
                    String::from_utf8_lossy(&eerr.stderr)
                )
            }
            LxcError::Start(ioerr) => ioerr.fmt(f),
        }
    }
}

fn run_lxc(args: &[&str]) -> Result<Vec<u8>, LxcError> {
    let res = Command::new("lxc").args(args).output()?;

    if !res.status.success() {
        return Err(LxcError::Execution(LxcCommandError {
            stderr: res.stderr,
            exit_code: res.status.code().unwrap_or(255),
        }));
    }
    return Ok(res.stdout);
}

fn main() {
    let res = run_lxc(&["foo"]);
    match res {
        Err(err) => eprintln!("cannot run lxc command: {}", err),
        Ok(out) => eprintln!("lxc output:\n{}", String::from_utf8_lossy(&out)),
    }
}
