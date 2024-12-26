use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::process::Command;

use log::debug;
use serde;
use serde_yml;

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
    Config(serde_yml::Error),
    Allocate(io::Error),
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
            LxcError::Config(srerr) => srerr.fmt(f),
            LxcError::Allocate(ioerr) => ioerr.fmt(f),
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
    image: &'a str,
    name: &'a str,
    cpu: u32,
    memory: u64,
    root_size: u64,
    secure_boot: bool,
}

pub trait LxdAllocatorExecutor {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<(), LxcError>;
    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError>;
    fn deallocate_all(&self) -> Result<(), LxcError>;
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
        let memory_arg = format!("limits.memory={}", node.memory);
        let cpu_arg = format!("limits.cpu={}", node.cpu);
        let secure_boot_arg = format!("security.secureboot={}", node.secure_boot);
        let root_size_arg = format!("root,size={}", node.root_size);
        let mut args = vec![
            "launch",
            "--ephemeral",
            "--vm",
            "--config",
            &memory_arg,
            "--config",
            &cpu_arg,
            "--config",
            &secure_boot_arg,
            "--device",
            &root_size_arg,
            node.image,
            node.name,
        ];
        run_lxc(&args).map(|_| ())
    }

    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        run_lxc(&["delete", "--force"]).map(|_| ())
    }

    fn deallocate_all(&self) -> Result<(), LxcError> {
        Err(LxcError::Other(io::Error::other("not implemented")))
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
    conf: LxdBackendConfig,
}

impl<A: LxdAllocatorExecutor> LxdAllocator<A> {
    pub fn allocate(&self, sysname: &str) -> Result<(), LxcError> {
        let sysconf = if let Some(sysconf) = self.conf.system.get(sysname) {
            sysconf
        } else {
            return Err(LxcError::Allocate(io::Error::other("system not found")));
        };

        if let Err(err) = self.backend.ensure_project(LXD_PROJECT_NAME) {
            return Err(err);
        }

        self.backend.allocate(&LxdNodeDetails {
            image: &sysconf.image,
            cpu: sysconf.resources.cpu,
            memory: 1024 * 1024 * 1024 * 2,
            name: sysname,
            root_size: 15 * 1024 * 1024 * 1024,
            secure_boot: sysconf.secure_boot,
        })
    }

    pub fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        self.backend.deallocate_by_addr(addr)
    }

    pub fn deallocate_all(&self) -> Result<(), LxcError> {
        self.backend.deallocate_all()
    }
}

#[derive(serde::Deserialize, Debug)]
struct LxdNodeResources {
    mem: String,
    cpu: u32,
    size: String,
}

#[derive(serde::Deserialize, Debug)]
struct LxdNodeConfig {
    image: String,
    #[serde(rename = "setup-steps")]
    setup_steps: String,
    resources: LxdNodeResources,
    #[serde(rename = "secure-boot", default)]
    secure_boot: bool,
}

#[derive(serde::Deserialize, Debug)]
struct LxdBackendConfig {
    system: HashMap<String, LxdNodeConfig>,
    setup: HashMap<String, Vec<String>>,
}

pub fn allocator_with_config<R>(cfg: R) -> Result<LxdAllocator<LxdCliAllocator>, LxcError>
where
    R: io::Read,
{
    let conf: LxdBackendConfig = serde_yml::from_reader(cfg).map_err(|e| LxcError::Config(e))?;
    log::debug!("config: {:?}", conf);

    Ok(LxdAllocator {
        conf: conf,
        backend: LxdCliAllocator {},
    })
}

pub fn config_file_name() -> &'static str {
    return "spread-lxd.yaml";
}
