// SPDX-FileCopyrightText: 2024 Maciej Borzecki <maciek.borzecki@gmail.com>
//
// SPDX-License-Identifier: MIT

use std::env;
use std::fs;
use std::io::Error;
use std::path::{Path, PathBuf};

use log;

const SPREAD_CONF_NAME: &'static str = "spread.yaml";

pub fn locate(name: &str) -> Result<PathBuf, Error> {
    let start_dir = &env::current_dir().and_then(|p| fs::canonicalize(p))?;
    let mut dir = Some(Path::new(start_dir));

    while let Some(curdir) = dir {
        log::debug!("checking {}", curdir.display());
        let backend_conf = curdir.join(name);
        let spread_conf = curdir.join(SPREAD_CONF_NAME);

        if spread_conf.exists() {
            log::debug!("found spread config {}", spread_conf.display());
            if !backend_conf.exists() {
                return Err(Error::other(format!(
                    "backend config file {} not found next to {}",
                    name,
                    spread_conf.display()
                )));
            } else {
                return Ok(backend_conf);
            }
        } else {
            dir = curdir.parent();
        }
    }
    return Err(Error::other(format!("cannot find {SPREAD_CONF_NAME}")));
}
