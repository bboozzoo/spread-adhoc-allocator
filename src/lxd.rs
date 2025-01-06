use core::net;
use core::time;
use std::collections::HashMap;
use std::io;
use std::process::Command;
use std::thread;
use std::time::Instant;

use log::debug;
use serde;
use serde_yml;
use thiserror;

#[derive(thiserror::Error, Debug)]
pub enum LxcError {
    #[error("cannot start lxc: {0}")]
    Start(io::Error),
    #[error("lxc command exited with status {exit_code}, stderr:\n{stderr}")]
    Execution { stderr: String, exit_code: i32 },
    #[error("error: {0}")]
    Other(io::Error),
    #[error("cannot load configuration: {0}")]
    Config(serde_yml::Error),
    #[error("cannot allocate system: {0}")]
    Allocate(io::Error),
    #[error("{0} not found")]
    NotFound(String),
}

pub struct LxdNodeAllocation {
    pub name: String,
    pub addr: net::Ipv4Addr,
    pub ssh_port: u32,
}

pub struct LxdNodeDetails<'a> {
    image: &'a str,
    name: &'a str,
    cpu: u32,
    memory: u64,
    root_size: u64,
    secure_boot: bool,
    provision_steps: &'a [String],
}

pub trait LxdAllocatorExecutor {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<LxdNodeAllocation, LxcError>;
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

    fn with_scope(scope: LxcRunnerScope<'a>) -> Self {
        LxcRunner { scope }
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, LxcError> {
        let mut cmd = Command::new("lxc");
        if let LxcRunnerScope::Project(prj) = &self.scope {
            cmd.arg("--project");
            cmd.arg(&prj);
        }

        cmd.args(args);

        log::trace!(
            "running lxc with: {:?}",
            cmd.get_args()
                .by_ref()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
        );

        let res = match cmd.output() {
            Ok(output) => output,
            Err(err) => {
                return Err(LxcError::Start(err));
            }
        };

        if !res.status.success() {
            return Err(LxcError::Execution {
                stderr: String::from_utf8_lossy(&res.stderr).trim().to_string(),
                exit_code: res.status.code().unwrap_or(255),
            });
        }
        return Ok(res.stdout);
    }
}

struct LxdCliAllocator;

mod lxc {
    pub mod types {
        use std::collections::HashMap;

        #[derive(serde::Deserialize, Debug, Clone)]
        pub struct NetworkAddress {
            pub family: String,
            pub address: String,
        }

        #[derive(serde::Deserialize, Debug, Clone)]
        pub struct NetworkState {
            pub addresses: Vec<NetworkAddress>,
        }

        #[derive(serde::Deserialize, Debug, Clone)]
        pub struct InstanceState {
            pub network: HashMap<String, NetworkState>,
        }

        #[derive(serde::Deserialize, Debug, Clone)]
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
        LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["list", "--format=json"])
            .and_then(|output| {
                Ok(serde_json::from_slice::<Vec<lxc::types::Instance>>(&output)
                    .expect("cannot parse instance list JSON"))
            })
    }

    fn list_node_by_name(name: &str) -> Result<lxc::types::Instance, LxcError> {
        let nodes = LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["list", "--format=json", name])
            .and_then(|output| {
                Ok(serde_json::from_slice::<Vec<lxc::types::Instance>>(&output)
                    .expect("cannot parse instance JSON"))
            })?;

        if nodes.len() == 0 {
            Err(LxcError::NotFound(name.to_string()))
        } else {
            Ok(nodes[0].clone())
        }
    }

    fn deallocate_by_name(name: &str) -> Result<(), LxcError> {
        log::debug!("deallocate by name '{}'", name);

        LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&["delete", "--force", name])
            .map(|_| ())
    }

    fn lxdfiy_name(name: &str) -> String {
        String::from_iter(name.chars().map(|c| match c {
            '.' => '-',
            _ => c,
        }))
    }

    fn wait_for_address(name: &str, timeout: time::Duration) -> Result<net::Ipv4Addr, LxcError> {
        let mut addr: Option<net::Ipv4Addr> = None;

        let now = Instant::now();

        while addr.is_none() {
            log::debug!("waiting for address");

            thread::sleep(time::Duration::from_millis(500));

            let instance = LxdCliAllocator::list_node_by_name(&name)?;
            if instance.status != "Running" {
                log::debug!("not yet running, in state {}", instance.status);
                continue;
            }

            for (ifname, ifstate) in instance.state.network.iter() {
                if ifname == "lo" {
                    continue;
                }

                for ifaceaddr in ifstate.addresses.iter() {
                    if ifaceaddr.family != "inet" {
                        continue;
                    }

                    log::debug!("found address {}", ifaceaddr.address);

                    if let Ok(parsed) = ifaceaddr.address.parse::<net::Ipv4Addr>() {
                        addr = Some(parsed);
                        break;
                    } else {
                        log::debug!("cannot parse address");
                    }
                }

                if addr.is_some() {
                    break;
                }
            }

            if addr.is_none() && now.elapsed() > timeout {
                return Err(LxcError::Allocate(io::Error::other(
                    "timeout waiting for instance to obtain an address",
                )));
            }
        }

        return Ok(addr.expect("address not set"));
    }

    fn provision(name: &str, steps: &[String]) -> Result<(), LxcError> {
        log::debug!("provision {}", name);

        let cli = LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME));
        for step in steps {
            log::debug!("provisioning step:\n{}", step);
            cli.run(&["exec", name, "--", "/bin/bash", "-c", step])?;
        }
        Ok(())
    }
}

impl LxdAllocatorExecutor for LxdCliAllocator {
    fn allocate(&self, node: &LxdNodeDetails) -> Result<LxdNodeAllocation, LxcError> {
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

        LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
            .run(&args)
            .map(|_| ())?;

        let addr = LxdCliAllocator::wait_for_address(&name, time::Duration::from_secs(60))?;

        LxdCliAllocator::provision(&name, node.provision_steps)?;

        Ok(LxdNodeAllocation {
            name: name,
            addr: addr,
            ssh_port: 22,
        })
    }

    fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        log::debug!("deallocate by address '{}'", addr);

        let nodes = LxdCliAllocator::list_nodes()?;

        let mut name: Option<String> = None;
        for instance in nodes.iter() {
            if instance.status != "Running" {
                continue;
            }

            let has_match = instance
                .state
                .network
                .iter()
                .find(|(_, iface)| {
                    iface
                        .addresses
                        .iter()
                        .find(|ifaceaddr| ifaceaddr.address == addr)
                        .is_some()
                })
                .is_some();

            if has_match {
                name = Some(instance.name.clone());
                break;
            }
        }

        if let Some(name) = name {
            LxdCliAllocator::deallocate_by_name(&name)
        } else {
            Err(LxcError::NotFound(addr.to_string()))
        }
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

        LxcRunner::with_scope(LxcRunnerScope::Project(LXD_PROJECT_NAME))
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

const LXD_PROJECT_NAME: &str = "spread-adhoc";

pub struct LxdAllocator {
    backend: Box<dyn LxdAllocatorExecutor>,
    conf: LxdBackendConfig,
}

pub struct UserConfig<'a> {
    pub user: &'a str,
    pub password: &'a str,
}

impl LxdAllocator {
    pub fn allocate(
        &self,
        sysname: &str,
        user_config: UserConfig,
    ) -> Result<LxdNodeAllocation, LxcError> {
        let sysconf = if let Some(sysconf) = self.conf.system.get(sysname) {
            sysconf
        } else {
            return Err(LxcError::Allocate(io::Error::other("system not found")));
        };

        let steps = if let Some(setup_steps) = sysconf.setup_steps.as_ref() {
            if let Some(steps) = self.conf.setup.get(setup_steps) {
                steps
            } else {
                return Err(LxcError::Other(io::Error::other("setup steps not found")));
            }
        } else {
            log::warn!("no setup steps declared for this system");
            &vec![]
        };

        // TODO validate user & password
        let mut steps = steps.clone();

        steps.push(format!(
            "echo {}:{} | chpasswd",
            user_config.user, user_config.password
        ));

        self.backend.ensure_project(LXD_PROJECT_NAME)?;

        self.backend.allocate(&LxdNodeDetails {
            image: &sysconf.image,
            cpu: sysconf.resources.cpu,
            memory: sysconf.resources.mem.as_u64(),
            name: sysname,
            root_size: sysconf.resources.size.as_u64(),
            secure_boot: sysconf.secure_boot,
            provision_steps: &steps,
        })
    }

    pub fn deallocate_by_addr(&self, addr: &str) -> Result<(), LxcError> {
        self.backend.deallocate_by_addr(addr)
    }

    pub fn deallocate_all(&self) -> Result<(), LxcError> {
        self.backend.deallocate_all()
    }

    pub fn new() -> Self {
        LxdAllocator {
            conf: LxdBackendConfig {
                setup: HashMap::new(),
                system: HashMap::new(),
            },
            backend: Box::new(LxdCliAllocator {}),
        }
    }

    fn new_with_config(conf: LxdBackendConfig) -> Self {
        LxdAllocator {
            conf: conf,
            backend: Box::new(LxdCliAllocator {}),
        }
    }
}

fn default_mem() -> bytesize::ByteSize {
    bytesize::ByteSize(bytesize::gib(2 as u64))
}

fn default_cpu() -> u32 {
    2
}

fn default_root_size() -> bytesize::ByteSize {
    bytesize::ByteSize(bytesize::gib(10 as u64))
}

#[derive(serde::Deserialize, Debug)]
struct LxdNodeResources {
    #[serde(default = "default_mem")]
    mem: bytesize::ByteSize,
    #[serde(default = "default_cpu")]
    cpu: u32,
    #[serde(default = "default_root_size")]
    size: bytesize::ByteSize,
}

impl Default for LxdNodeResources {
    fn default() -> Self {
        LxdNodeResources {
            mem: default_mem(),
            cpu: default_cpu(),
            size: default_root_size(),
        }
    }
}

#[derive(serde::Deserialize, Debug)]
struct LxdNodeConfig {
    image: String,
    #[serde(rename = "setup-steps")]
    setup_steps: Option<String>,
    #[serde(default)]
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

    Ok(LxdAllocator::new_with_config(conf))
}

pub fn config_file_name() -> &'static str {
    return "spread-lxd.yaml";
}
