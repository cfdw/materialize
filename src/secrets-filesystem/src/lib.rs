// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

#![deny(missing_docs)]

//! Secrets management via the local filesystem.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use mz_repr::GlobalId;
use mz_secrets::{SecretsController, SecretsReader};

/// A [`SecretsController`] that stores secrets in plaintext on the local
/// filesystem.
///
/// For use only in development and testing.
#[derive(Debug)]
pub struct FilesystemSecretsController {
    path: PathBuf,
}

impl FilesystemSecretsController {
    /// Constructs a new filesystem secrets controller.
    ///
    /// Secrets will be stored at the specified path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl SecretsController for FilesystemSecretsController {
    async fn ensure(&mut self, id: GlobalId, contents: &[u8]) -> Result<(), anyhow::Error> {
        let file_path = self.path.join(id.to_string());
        let mut file = OpenOptions::new()
            .mode(0o600)
            .create(true)
            .write(true)
            .truncate(true)
            .open(file_path)
            .await?;
        file.write_all(contents).await?;
        file.sync_all().await?;
        Ok(())
    }

    async fn delete(&mut self, id: GlobalId) -> Result<(), anyhow::Error> {
        fs::remove_file(self.path.join(id.to_string())).await?;
        Ok(())
    }
}

/// A [`SecretsReader`] that reads secrets managed by a
/// [`FilesystemSecretsController`].
#[derive(Debug)]
pub struct FilesystemSecretsReader {
    path: PathBuf,
}

impl FilesystemSecretsReader {
    /// Constructs a new filesystem secrets reader.
    ///
    /// Secrets will be read from the specified path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl SecretsReader for FilesystemSecretsReader {
    async fn read(&self, id: GlobalId) -> Result<Vec<u8>, anyhow::Error> {
        let contents = fs::read(self.path.join(id.to_string())).await?;
        Ok(contents)
    }
}
