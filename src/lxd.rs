use std::error::Error;
use std::fmt;
use std::io;
use std::process::Command;

use log::debug;
use serde;

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

struct LxdNodeDetails<'a> {
    name: &'a str,
    CPUs: u32,
    memory: u32,
}

pub trait LxdAllocatorExecutor {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<(), LxcError>;
    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError>;
    fn ensure_project(&self, project: &str) -> Result<(), LxcError>;
}

pub struct LxdCliAllocator {}

impl LxdCliAllocator {
    fn add_project(&self, project: &str) -> Result<(), LxcError> {
        run_lxc(&["project", "create", project]).map(|_| ())
    }

    fn find_project(&self, project: &str, output: &Vec<u8>) -> Result<bool, LxcError> {
        #[derive(serde::Deserialize, Debug)]
        struct _LxcProject {
            name: String,
        }

        let projects: Vec<_LxcProject> =
            serde_json::from_slice(output).expect("cannot parse project JSON");

        debug!("projects:\n{:?}", projects);

        let found = projects.iter().find(|p| p.name == project).is_some();
        debug!("project found? {}", found);
        return Ok(found);
    }
}

impl LxdAllocatorExecutor for LxdCliAllocator {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<(), LxcError> {
        run_lxc(&["launch"]).map(|_| ())
    }

    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        run_lxc(&["delete", "--force"]).map(|_| ())
    }

    fn ensure_project(&self, project: &str) -> Result<(), LxcError> {
        run_lxc(&["project", "list", "--format=json"])
            .and_then(|out| self.find_project(project, &out))
            .and_then(|found| {
                if !found {
                    self.add_project(project)
                } else {
                    Ok(())
                }
            })
    }
}

const LXD_PROJECT_NAME: &'static str = "spread-adhoc";

pub struct LxdAllocator<A: LxdAllocatorExecutor> {
    backend: A,
}

impl<A: LxdAllocatorExecutor> LxdAllocator<A> {
    pub fn allocate(&self, sysname: &str) -> Result<(), LxcError> {
        if let Err(err) = self.backend.ensure_project(LXD_PROJECT_NAME) {
            return Err(err);
        }

        self.backend.allocate(&LxdNodeDetails {
            CPUs: 2,
            memory: 1024 * 1024 * 1024 * 2,
            name: sysname,
        })
    }

    pub fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        self.backend.deallocate_by_addr(addr)
    }
}

pub fn allocator() -> LxdAllocator<LxdCliAllocator> {
    LxdAllocator {
        backend: LxdCliAllocator {},
    }
}
