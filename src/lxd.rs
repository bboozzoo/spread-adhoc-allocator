use std::error::Error;
use std::fmt;
use std::io;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct LxcCommandError {
    stderr: Vec<u8>,
    exit_code: i32,
}

#[derive(Debug)]
pub enum LxcError {
    Start(io::Error),
    Execution(LxcCommandError),
    Other(io::Error),
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
            LxcError::Other(ioerr) => ioerr.fmt(f),
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

trait LxdAllocator {
    fn allocate(&self) -> Result<(), LxcError>;
    fn deallocate(&self, addr: &str) -> Result<(), LxcError>;
    fn ensure_project(&self, project: &str) -> Result<(), LxcError>;
}

struct LxdCliALlocator {}

impl LxdCliALlocator {
    fn add_project(&self, project: &str) -> Result<(), LxcError> {
        run_lxc(&["project", "add", project]).map(|_| ())
    }

    fn find_project(&self, project: &str, output: &Vec<u8>) -> Result<bool, LxcError> {
        Err(LxcError::Other(io::Error::other("mock")))
    }
}

impl LxdAllocator for LxdCliALlocator {
    fn allocate(&self) -> Result<(), LxcError> {
        run_lxc(&["launch"]).map(|_| ())
    }
    fn deallocate(&self, addr: &str) -> Result<(), LxcError> {
        run_lxc(&["delete", "--force"]).map(|_| ())
    }

    fn ensure_project(&self, project: &str) -> Result<(), LxcError> {
        run_lxc(&["project", "list", "--format=json"])
            .and_then(|out| self.find_project(project, &out))
            .and_then(|found| {
                if found {
                    self.add_project(project)
                } else {
                    Ok(())
                }
            })
    }
}

fn default_allocator() -> LxdCliALlocator {
    LxdCliALlocator {}
}

pub fn allocate(sysname: &str) -> Result<(), LxcError> {
    default_allocator().allocate()
}

pub fn deallocate_by_addr(addr: &str) -> Result<(), LxcError> {
    default_allocator().deallocate(addr)
}
