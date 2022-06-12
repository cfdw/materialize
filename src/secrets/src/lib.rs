// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

#![deny(missing_docs)]

//! Abstractions for secure management of secrets.

use std::fmt;

use anyhow::Context;
use async_trait::async_trait;

use mz_repr::GlobalId;

/// Securely stores secrets.
#[async_trait]
pub trait SecretsController: fmt::Debug + Send + Sync {
    /// Creates or updates the specified secret with the specified binary
    /// contents.
    async fn ensure(&mut self, id: GlobalId, contents: &[u8]) -> Result<(), anyhow::Error>;

    /// Deletes the specified secret.
    async fn delete(&mut self, id: GlobalId) -> Result<(), anyhow::Error>;
}

/// Securely reads secrets that are managed by a [`SecretsController`].
///
/// Does not provide access to create, update, or delete the secrets within.
#[async_trait]
pub trait SecretsReader: fmt::Debug + Send + Sync {
    /// Returns the binary contents of the specified secret.
    async fn read(&self, id: GlobalId) -> Result<Vec<u8>, anyhow::Error>;

    /// Returns the string contents of the specified secret.
    ///
    /// Returns an error if the secret's contents cannot be decoded as UTF-8.
    async fn read_string(&self, id: GlobalId) -> Result<String, anyhow::Error> {
        let contents = self.read(id).await?;
        String::from_utf8(contents).context("converting secret value to string")
    }
}
