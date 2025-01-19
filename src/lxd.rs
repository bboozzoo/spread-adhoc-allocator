// SPDX-FileCopyrightText: 2024 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use core::net;
use core::time;
use std::collections::HashMap;
use std::io;
use std::process::Command;
use std::thread;
use std::time::Instant;

use log::debug;
use rand::random;
use serde;
use serde_yml;
use thiserror;

/// Wraps LXD executor errors.
#[derive(thiserror::Error, Debug)]
pub enum LxdError {
    #[error("cannot execute operation: {0}")]
    Executor(String),
    #[error("cannot load configuration: {0}")]
    Config(serde_yml::Error),
    #[error("cannot allocate system: {0}")]
    Allocate(String),
    #[error("cannot deallocate system: {0}")]
    Deallocate(String),
    #[error("{0}")]
    NotFound(String),
}

/// Describes an allocate lxd node.
pub struct LxdNodeAllocation {
    pub name: String,
    pub addr: net::Ipv4Addr,
    pub ssh_port: u32,
}

/// Carries details of a node to allocate.
pub struct LxdNodeDetails<'a> {
    image: &'a str,
    name: &'a str,
    cpu: u32,
    memory: u64,
    root_size: u64,
    secure_boot: bool,
    provision_steps: &'a [String],
}

/// An executor for allocating nodes using LXD.
pub trait LxdAllocatorExecutor {
    /// Allocate a node with given confuguration.
    fn allocate(&mut self, node: &LxdNodeDetails) -> Result<LxdNodeAllocation, LxdError>;
    /// Deallocate a node with given address.
    fn deallocate_by_addr(&mut self, addr: &str) -> Result<(), LxdError>;
    /// Deallocate all nodes.
    fn deallocate_all(&mut self) -> Result<(), LxdError>;
    /// Ensure a given LXD project exists.
    fn ensure_project(&mut self, project: &str) -> Result<(), LxdError>;
}

struct LxcCommand(Command);

/// Scope for lxc commands.
enum LxcCommandScope<'a> {
    /// Default project.
    Default,
    /// Specific project.
    Project(&'a str),
}

/// Builds lxc command line.
struct LxcCommandBuilder<'a> {
    scope: LxcCommandScope<'a>,
    args: Vec<&'a str>,
}

impl<'a> LxcCommandBuilder<'a> {
    fn new() -> Self {
        Self {
            scope: LxcCommandScope::Default,
            args: Vec::new(),
        }
    }

    fn with_scope(mut self, scope: LxcCommandScope<'a>) -> Self {
        self.scope = scope;
        self
    }

    fn args(mut self, args: &'a [&str]) -> Self {
        self.args = args.to_vec();
        self
    }

    fn build(self) -> LxcCommand {
        let mut cmd = Command::new("lxc");
        if let LxcCommandScope::Project(prj) = &self.scope {
            cmd.arg("--project");
            cmd.arg(&prj);
        }

        cmd.args(self.args);
        LxcCommand(cmd)
    }
}

/// Wraps lxc command runner errors.
#[derive(thiserror::Error, Debug)]
pub enum LxcRunnerError {
    #[error("cannot start lxc: {0}")]
    Start(io::Error),
    #[error("lxc command exited with status {exit_code}, stderr:\n{stderr}")]
    Execution { stderr: String, exit_code: i32 },
}

/// Trait representing a way to run lxc command.
trait LxcRunner {
    fn run(&mut self, cmd: LxcCommand) -> Result<Vec<u8>, LxcRunnerError>;
}

/// Wrapper for runing lxc commands.
struct LxcCommandRunner;

impl LxcRunner for LxcCommandRunner {
    /// Runs a command returning its output (stdout).
    fn run(&mut self, lxccmd: LxcCommand) -> Result<Vec<u8>, LxcRunnerError> {
        let LxcCommand(mut cmd) = lxccmd;

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
                return Err(LxcRunnerError::Start(err));
            }
        };

        if !res.status.success() {
            return Err(LxcRunnerError::Execution {
                stderr: String::from_utf8_lossy(&res.stderr).trim().to_string(),
                exit_code: res.status.code().unwrap_or(255),
            });
        }
        return Ok(res.stdout);
    }
}

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
            pub network: Option<HashMap<String, NetworkState>>,
        }

        #[derive(serde::Deserialize, Debug, Clone)]
        pub struct Instance {
            pub name: String,
            pub state: InstanceState,
            pub status: String,
        }
    }
}

fn lxdfy_name(name: &str) -> String {
    String::from_iter(name.chars().map(|c| match c {
        '.' | ':' => '-',
        _ => c,
    }))
}

/// Wraps lxc backend executor errors.
#[derive(thiserror::Error, Debug)]
pub enum LxcCliAllocatorError {
    #[error("cannot add project: {0}")]
    AddProject(String),
    #[error("cannot list nodes: {0}")]
    ListNodes(String),
    #[error("cannot find node")]
    NodeNotFound,
    #[error("cannot delete node: {0}")]
    DeleteNode(String),
    #[error("cannot obtain address")]
    AddressTimeout,
    #[error("cannot provision node: {0}")]
    Provision(String),
}

/// Lxd node allocator which uses 'lxc' command.
struct LxdCliAllocator<R>
where
    R: LxcRunner,
{
    runner: R,
}

impl<R> LxdCliAllocator<R>
where
    R: LxcRunner,
{
    fn new(r: R) -> Self {
        Self { runner: r }
    }

    fn add_project(&mut self, project: &str) -> Result<(), LxcCliAllocatorError> {
        self.runner
            .run(
                LxcCommandBuilder::new()
                    .args(&[
                        "project",
                        "create",
                        project,
                        "-c",
                        "features.images=false",
                        "-c",
                        "features.profiles=false",
                    ])
                    .build(),
            )
            .map(|_| ())
            .map_err(|e| LxcCliAllocatorError::AddProject(e.to_string()))
    }

    fn list_nodes(&mut self) -> Result<Vec<lxc::types::Instance>, LxcCliAllocatorError> {
        self.runner
            .run(
                LxcCommandBuilder::new()
                    .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                    .args(&["list", "--format=json"])
                    .build(),
            )
            .map_err(|e| LxcCliAllocatorError::ListNodes(e.to_string()))
            .and_then(|output| {
                Ok(serde_json::from_slice::<Vec<lxc::types::Instance>>(&output)
                    .expect("cannot parse instance list JSON"))
            })
    }

    fn list_node_by_name(
        &mut self,
        name: &str,
    ) -> Result<lxc::types::Instance, LxcCliAllocatorError> {
        let nodes = self
            .runner
            .run(
                LxcCommandBuilder::new()
                    .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                    .args(&["list", "--format=json", name])
                    .build(),
            )
            .map_err(|e| LxcCliAllocatorError::ListNodes(e.to_string()))
            .and_then(|output| {
                Ok(serde_json::from_slice::<Vec<lxc::types::Instance>>(&output)
                    .expect("cannot parse instance JSON"))
            })?;

        if nodes.len() == 0 {
            Err(LxcCliAllocatorError::NodeNotFound)
        } else {
            Ok(nodes[0].clone())
        }
    }

    fn deallocate_by_name(&mut self, name: &str) -> Result<(), LxcCliAllocatorError> {
        log::debug!("deallocate by name '{}'", name);

        self.runner
            .run(
                LxcCommandBuilder::new()
                    .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                    .args(&["delete", "--force", name])
                    .build(),
            )
            .map_err(|e| LxcCliAllocatorError::DeleteNode(e.to_string()))
            .map(|_| ())
    }

    fn wait_for_address(
        &mut self,
        name: &str,
        timeout: time::Duration,
    ) -> Result<net::Ipv4Addr, LxcCliAllocatorError> {
        let mut addr: Option<net::Ipv4Addr> = None;

        let now = Instant::now();

        while addr.is_none() {
            log::debug!("waiting for address");

            thread::sleep(time::Duration::from_millis(500));

            let instance = self.list_node_by_name(&name)?;
            if instance.status != "Running" {
                log::debug!("not yet running, in state {}", instance.status);
                continue;
            }

            for (ifname, ifstate) in instance.state.network.unwrap_or(HashMap::new()).iter() {
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
                return Err(LxcCliAllocatorError::AddressTimeout);
            }
        }

        return Ok(addr.expect("address not set"));
    }

    fn provision(&mut self, name: &str, steps: &[String]) -> Result<(), LxcCliAllocatorError> {
        log::debug!("provision {}", name);

        for step in steps {
            log::debug!("provisioning step:\n{}", step);
            self.runner
                .run(
                    LxcCommandBuilder::new()
                        .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                        .args(&["exec", name, "--", "/bin/bash", "-c", step])
                        .build(),
                )
                .map_err(|e| LxcCliAllocatorError::Provision(e.to_string()))?;
        }
        Ok(())
    }
}

impl<R> LxdAllocatorExecutor for LxdCliAllocator<R>
where
    R: LxcRunner,
{
    fn allocate(&mut self, node: &LxdNodeDetails) -> Result<LxdNodeAllocation, LxdError> {
        let memory_arg = format!("limits.memory={}", node.memory);
        let cpu_arg = format!("limits.cpu={}", node.cpu);
        let secure_boot_arg = format!("security.secureboot={}", node.secure_boot);
        let root_size_arg = format!("root,size={}", node.root_size);
        let name = lxdfy_name(node.name);
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

        self.runner
            .run(
                LxcCommandBuilder::new()
                    .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                    .args(&args)
                    .build(),
            )
            .map_err(|e| LxdError::Allocate(e.to_string()))
            .map(|_| ())?;

        let addr = self
            .wait_for_address(&name, time::Duration::from_secs(60))
            .map_err(|e| LxdError::Allocate(e.to_string()))?;

        self.provision(&name, node.provision_steps)
            .map_err(|e| LxdError::Allocate(e.to_string()))?;

        Ok(LxdNodeAllocation {
            name: name,
            addr: addr,
            ssh_port: 22,
        })
    }

    fn deallocate_by_addr(&mut self, addr: &str) -> Result<(), LxdError> {
        log::debug!("deallocate by address '{}'", addr);

        let nodes = self
            .list_nodes()
            .map_err(|e| LxdError::Deallocate(e.to_string()))?;

        let mut name: Option<String> = None;
        for instance in nodes.iter() {
            if instance.status != "Running" {
                continue;
            }

            let has_match = instance
                .state
                .network
                .as_ref()
                .unwrap_or(&HashMap::new())
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
            self.deallocate_by_name(&name)
                .map_err(|e| LxdError::Deallocate(e.to_string()))
        } else {
            Err(LxdError::NotFound(addr.to_string()))
        }
    }

    fn deallocate_all(&mut self) -> Result<(), LxdError> {
        let nodes = self
            .list_nodes()
            .map_err(|e| LxdError::Deallocate(e.to_string()))?;
        log::debug!("deallocate {} nodes: {:?}", nodes.len(), nodes);

        for node in nodes {
            self.deallocate_by_name(&node.name)
                .map_err(|e| LxdError::Deallocate(e.to_string()))?;
        }

        Ok(())
    }

    fn ensure_project(&mut self, project: &str) -> Result<(), LxdError> {
        #[derive(serde::Deserialize, Debug)]
        struct _LxcProject {
            name: String,
        }

        let found = self
            .runner
            .run(
                LxcCommandBuilder::new()
                    .with_scope(LxcCommandScope::Project(LXD_PROJECT_NAME))
                    .args(&["project", "list", "--format=json"])
                    .build(),
            )
            .and_then(|output| {
                let found = serde_json::from_slice::<Vec<_LxcProject>>(&output)
                    .expect("cannot parse project JSON")
                    .iter()
                    .find(|p| p.name == project)
                    .is_some();

                debug!("project found {}", found);

                Ok(found)
            })
            .map_err(|e| LxdError::Executor(e.to_string()))?;

        if !found {
            self.add_project(project)
                .map_err(|e| LxdError::Executor(e.to_string()))
        } else {
            Ok(())
        }
    }
}

const LXD_PROJECT_NAME: &str = "spread-adhoc";

/// Spread node allocator using LXD backend.
pub struct LxdAllocator {
    backend: Box<dyn LxdAllocatorExecutor>,
    conf: LxdBackendConfig,
}

/// Carries details for confugration of remote user access.
pub struct RemoteUserAccessConfig<'a> {
    pub user: &'a str,
    pub password: &'a str,
}

impl LxdAllocator {
    /// Allocate a node for a spread system and set up remote access for the
    /// user.
    pub fn allocate(
        &mut self,
        sysname: &str,
        user_config: RemoteUserAccessConfig,
    ) -> Result<LxdNodeAllocation, LxdError> {
        let sysconf = if let Some(sysconf) = self.conf.system.get(sysname) {
            sysconf
        } else {
            return Err(LxdError::NotFound(format!(
                "system \"{}\" not found in configuration",
                sysname
            )));
        };

        let steps = if let Some(setup_steps) = sysconf.setup_steps.as_ref() {
            if let Some(steps) = self.conf.setup.get(setup_steps) {
                steps
            } else {
                return Err(LxdError::NotFound(format!(
                    "setup steps \"{}\" not found in configuration",
                    setup_steps
                )));
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

        let name = format!("{}-{}", sysname, random::<u32>());

        self.backend.ensure_project(LXD_PROJECT_NAME)?;

        self.backend.allocate(&LxdNodeDetails {
            image: &sysconf.image,
            cpu: sysconf.resources.cpu,
            memory: sysconf.resources.mem.as_u64(),
            name: &name,
            root_size: sysconf.resources.size.as_u64(),
            secure_boot: sysconf.secure_boot,
            provision_steps: &steps,
        })
    }

    /// Deallocate a node associated with a given address.
    pub fn deallocate_by_addr(&mut self, addr: &str) -> Result<(), LxdError> {
        self.backend.deallocate_by_addr(addr)
    }

    /// Deallocate all nodes.
    pub fn deallocate_all(&mut self) -> Result<(), LxdError> {
        self.backend.deallocate_all()
    }

    /// Returns a new, unconfigured allocator.
    pub fn new() -> Self {
        LxdAllocator {
            conf: LxdBackendConfig {
                setup: HashMap::new(),
                system: HashMap::new(),
            },
            backend: Box::new(LxdCliAllocator::<LxcCommandRunner>::new(
                LxcCommandRunner {},
            )),
        }
    }

    fn new_with_config(conf: LxdBackendConfig) -> Self {
        LxdAllocator {
            conf: conf,
            backend: Box::new(LxdCliAllocator::<LxcCommandRunner>::new(
                LxcCommandRunner {},
            )),
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

/// Resources assigned to a node.
#[derive(serde::Deserialize, Debug)]
struct LxdNodeResources {
    /// RAM
    #[serde(default = "default_mem")]
    mem: bytesize::ByteSize,
    /// Number of CPUs.
    #[serde(default = "default_cpu")]
    cpu: u32,
    /// Root disk size (applicable to VM).
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

/// Configuration for a new LXD node.
#[derive(serde::Deserialize, Debug)]
struct LxdNodeConfig {
    /// Image to use.
    image: String,
    /// Setup steps.
    #[serde(rename = "setup-steps")]
    setup_steps: Option<String>,
    /// Resources configuration.
    #[serde(default)]
    resources: LxdNodeResources,
    /// Secure boot support (applicable to VMs).
    #[serde(rename = "secure-boot", default)]
    secure_boot: bool,
    /// Whether the system is a VM.
    #[serde(default)]
    vm: bool,
}

/// Configuration for the LXD backend.
#[derive(serde::Deserialize, Debug)]
struct LxdBackendConfig {
    /// Systems with their properties, keyed by spread system name.
    system: HashMap<String, LxdNodeConfig>,
    /// Setup steps.
    setup: HashMap<String, Vec<String>>,
}

/// Returns LXD node allocator loading its confugiration from the provided
/// reader.
pub fn allocator_with_config<R>(cfg: R) -> Result<LxdAllocator, LxdError>
where
    R: io::Read,
{
    let conf: LxdBackendConfig = serde_yml::from_reader(cfg).map_err(|e| LxdError::Config(e))?;
    log::debug!("config: {:?}", conf);

    Ok(LxdAllocator::new_with_config(conf))
}

/// Returns the file name of a LXD node allocator.
pub fn config_file_name() -> &'static str {
    return "spread-lxd.yaml";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{RefCell, RefMut};
    use std::rc::Rc;

    struct MockLxcRunner {
        v: Vec<Vec<String>>,
        r: Vec<Result<Vec<u8>, LxcRunnerError>>,
    }

    impl MockLxcRunner {
        fn new(calls: Vec<Result<Vec<u8>, LxcRunnerError>>) -> Self {
            Self {
                v: Vec::new(),
                r: calls,
            }
        }
    }

    impl LxcRunner for RefMut<'_, MockLxcRunner> {
        fn run(&mut self, cmd: LxcCommand) -> Result<Vec<u8>, LxcRunnerError> {
            let LxcCommand(cmd) = cmd;
            let call = cmd
                .get_args()
                .by_ref()
                .map(|v| v.to_string_lossy().to_string())
                .collect();

            let out = self
                .r
                .pop()
                .expect(&format!("expected mock result for call {:?}", call));

            self.v.push(call);
            out
        }
    }

    #[test]
    fn test_cli_alloc_add_project() {
        let r = Rc::new(RefCell::new(MockLxcRunner::new(vec![Ok(""
            .as_bytes()
            .to_vec())])));
        {
            let mut a = LxdCliAllocator::new(r.borrow_mut());
            let res = a.add_project("foo");
            assert!(res.is_ok());
        }
        let mut r = r.borrow_mut();
        assert_eq!(r.v.len(), 1);
        assert_eq!(
            r.v.pop().expect("expected a call"),
            vec![
                "project",
                "create",
                "foo",
                "-c",
                "features.images=false",
                "-c",
                "features.profiles=false"
            ]
        );
    }

    #[test]
    fn test_lxdify() {
        assert_eq!(lxdfy_name("foo-bar"), "foo-bar");
        assert_eq!(lxdfy_name("foo.bar"), "foo-bar");
        assert_eq!(lxdfy_name("foo:bar"), "foo-bar");
    }
}
