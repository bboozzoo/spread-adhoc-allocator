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
    NotFound,
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
            LxcError::NotFound => write!(f, "entry not found"),
        }
    }
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

enum LxcRunnerScope<'a> {
    Global,
    Project(&'a str),
}

struct LxcRunner<'a> {
    scope: LxcRunnerScope<'a>,
}

impl<'a> LxcRunner<'a> {
    fn new() -> Self {
        LxcRunner {
            scope: LxcRunnerScope::Global,
        }
    }

    fn with_scope(&mut self, scope: LxcRunnerScope<'a>) -> &mut Self {
        self.scope = scope;
        self
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, LxcError> {
        let mut cmd = Command::new("lxc");
        if let LxcRunnerScope::Project(prj) = &self.scope {
            cmd.arg("--project");
            cmd.arg(&prj);
        }

        cmd.args(args);

        let res = cmd.output()?;

        if !res.status.success() {
            return Err(LxcError::Execution(LxcCommandError {
                stderr: res.stderr,
                exit_code: res.status.code().unwrap_or(255),
            }));
        }
        return Ok(res.stdout);
    }
}

struct LxdCliAllocator;

mod lxc {
    pub mod types {
        use std::collections::HashMap;

        #[derive(serde::Deserialize, Debug)]
        pub struct NetworkAddress {
            pub family: String,
            pub address: String,
        }

        #[derive(serde::Deserialize, Debug)]
        pub struct NetworkState {
            pub addresses: Vec<NetworkAddress>,
        }

        #[derive(serde::Deserialize, Debug)]
        pub struct InstanceState {
            pub network: HashMap<String, NetworkState>,
        }

        #[derive(serde::Deserialize, Debug)]
        pub struct Instance {
            pub name: String,
            pub state: InstanceState,
            pub status: String,
        }
    }
}

impl LxdCliAllocator {
    fn add_project(project: &str) -> Result<(), LxcError> {
        LxcRunner::new()
            .run(&[
                "project",
                "create",
                project,
                "-c",
                "features.images=false",
                "-c",
                "features.profiles=false",
            ])
            .map(|_| ())
    }

    fn list_nodes() -> Result<Vec<lxc::types::Instance>, LxcError> {
        LxcRunner::new()
            .with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["list", "--format=json"])
            .and_then(|output| {
                Ok(serde_json::from_slice::<Vec<lxc::types::Instance>>(&output)
                    .expect("cannot parse instance list JSON"))
            })
    }

    fn deallocate_by_name(name: &str) -> Result<(), LxcError> {
        LxcRunner::new()
            .with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["delete", "--force", name])
            .map(|_| ())
    }

    fn lxdfiy_name(name: &str) -> String {
        String::from_iter(name.chars().map(|c| match c {
            '.' => '-',
            _ => c,
        }))
    }
}

impl LxdAllocatorExecutor for LxdCliAllocator {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<(), LxcError> {
        let memory_arg = format!("limits.memory={}", node.memory);
        let cpu_arg = format!("limits.cpu={}", node.cpu);
        let secure_boot_arg = format!("security.secureboot={}", node.secure_boot);
        let root_size_arg = format!("root,size={}", node.root_size);
        let name = LxdCliAllocator::lxdfiy_name(node.name);
        let args = vec![
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
            &name,
        ];

        LxcRunner::new()
            .with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&args)
            .map(|_| ())

        // TODO wait for node to become active
    }

    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        LxdCliAllocator::list_nodes().and_then(|instances| {
            let res = instances.iter().find(|instance| {
                instance.status == "Running"
                    && instance
                        .state
                        .network
                        .iter()
                        .find(|(_, &ref iface)| {
                            iface
                                .addresses
                                .iter()
                                .find(|ifaceaddr| ifaceaddr.address == addr)
                                .is_some()
                        })
                        .is_some()
            });

            match res {
                None => Err(LxcError::NotFound),
                Some(instance) => LxdCliAllocator::deallocate_by_name(&instance.name),
            }
        })
    }

    fn deallocate_all(&self) -> Result<(), LxcError> {
        let nodes = LxdCliAllocator::list_nodes()?;
        log::debug!("deallocate {} nodes: {:?}", nodes.len(), nodes);

        for node in nodes {
            LxdCliAllocator::deallocate_by_name(&node.name)?;
        }

        Ok(())
    }

    fn ensure_project(&self, project: &str) -> Result<(), LxcError> {
        #[derive(serde::Deserialize, Debug)]
        struct _LxcProject {
            name: String,
        }

        LxcRunner::new()
            .with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["project", "list", "--format=json"])
            .and_then(|output| {
                let found = serde_json::from_slice::<Vec<_LxcProject>>(&output)
                    .expect("cannot parse project JSON")
                    .iter()
                    .find(|p| p.name == project)
                    .is_some();

                debug!("project found {}", found);

                Ok(found)
            })
            .and_then(|found| {
                if !found {
                    LxdCliAllocator::add_project(project)
                } else {
                    Ok(())
                }
            })
    }
}

const LXD_PROJECT_NAME: &'static str = "spread-adhoc";

pub struct LxdAllocator {
    backend: Box<dyn LxdAllocatorExecutor>,
    conf: LxdBackendConfig,
}

impl LxdAllocator {
    pub fn allocate(&self, sysname: &str) -> Result<String, LxcError> {
        let sysconf = if let Some(sysconf) = self.conf.system.get(sysname) {
            sysconf
        } else {
            return Err(LxcError::Allocate(io::Error::other("system not found")));
        };

        if let Err(err) = self.backend.ensure_project(LXD_PROJECT_NAME) {
            return Err(err);
        }

        self.backend
            .allocate(&LxdNodeDetails {
                image: &sysconf.image,
                cpu: sysconf.resources.cpu,
                memory: 1024 * 1024 * 1024 * 2,
                name: sysname,
                root_size: 15 * 1024 * 1024 * 1024,
                secure_boot: sysconf.secure_boot,
            })
            .and_then(|_| Ok(String::new()))
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

pub fn allocator_with_config<R>(cfg: R) -> Result<LxdAllocator, LxcError>
where
    R: io::Read,
{
    let conf: LxdBackendConfig = serde_yml::from_reader(cfg).map_err(|e| LxcError::Config(e))?;
    log::debug!("config: {:?}", conf);

    Ok(LxdAllocator {
        conf: conf,
        backend: Box::new(LxdCliAllocator {}),
    })
}

pub fn config_file_name() -> &'static str {
    return "spread-lxd.yaml";
}
