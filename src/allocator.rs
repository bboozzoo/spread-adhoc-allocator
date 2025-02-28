// SPDX-FileCopyrightText: 2025 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use core::net;

use thiserror;

/// Describes allocated node.
pub struct Node {
    pub addr: net::Ipv4Addr,
    pub ssh_port: u32,
}

/// Carries details for confugration of remote user access.
pub struct RemoteUserAccessConfig<'a> {
    pub user: &'a str,
    pub password: &'a str,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("cannot execute operation: {0}")]
    Operation(String),
    #[error("{0}")]
    NotFound(String),
}

pub trait NodeAllocator {
    /// Allocate a node using a system name.
    fn allocate_by_name(
        &mut self,
        name: &str,
        user_config: RemoteUserAccessConfig,
    ) -> Result<Node, Error>;
    /// Discard a node with given address.
    fn discard_by_addr(&mut self, addr: &str) -> Result<(), Error>;
    /// Discard all nodes.
    fn discard_all(&mut self) -> Result<(), Error>;
}
