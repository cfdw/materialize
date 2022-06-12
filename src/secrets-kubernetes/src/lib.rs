// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

#![deny(missing_docs)]

//! Secrets management via Kubernetes.

use std::iter;

use anyhow::anyhow;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::ByteString;
use kube::api::{DeleteParams, Patch, PatchParams};
use kube::Api;

use mz_repr::GlobalId;
use mz_secrets::{SecretsController, SecretsReader};

/// Configures a [`KubernetesSecretsController`].
#[derive(Debug, Clone)]
pub struct KubernetesSecretsControllerConfig {
    /// The name of a Kubernetes context to use, if the Kubernetes configuration
    /// is loaded from the local kubeconfig.
    pub context: String,
    /// The field manager to present as when interacting with the Kubernetes
    /// API.
    pub field_manager: String,
}

/// A [`SecretsController`] that stores secrets as Kubernetes `Secret` objects.
///
/// Secrets are named `user-managed-ID` and contain one entry named `contents`
/// which stores the binary contents of the secret.
#[derive(Debug)]
pub struct KubernetesSecretsController {
    secret_api: Api<Secret>,
    field_manager: String,
}

impl KubernetesSecretsController {
    /// Constructs a new Kubernetes secrets controller from the provided
    /// configuration.
    pub async fn new(
        config: KubernetesSecretsControllerConfig,
    ) -> Result<KubernetesSecretsController, anyhow::Error> {
        let (client, _) = mz_kubernetes_util::create_client(config.context).await?;
        let secret_api: Api<Secret> = Api::default_namespaced(client);
        Ok(KubernetesSecretsController {
            secret_api,
            field_manager: config.field_manager,
        })
    }
}

#[async_trait]
impl SecretsController for KubernetesSecretsController {
    async fn ensure(&mut self, id: GlobalId, contents: &[u8]) -> Result<(), anyhow::Error> {
        let data = iter::once(("contents".into(), ByteString(contents.into())));
        let secret = Secret {
            data: Some(data.collect()),
            ..Default::default()
        };
        self.secret_api
            .patch(
                &secret_name(id),
                &PatchParams::apply(&self.field_manager).force(),
                &Patch::Apply(secret),
            )
            .await?;
        Ok(())
    }

    async fn delete(&mut self, id: GlobalId) -> Result<(), anyhow::Error> {
        self.secret_api
            .delete(&secret_name(id), &DeleteParams::default())
            .await?;
        Ok(())
    }
}

/// Reads secrets managed by a [`KubernetesSecretsController`].
#[derive(Debug)]
pub struct KubernetesSecretsReader {
    secret_api: Api<Secret>,
}

impl KubernetesSecretsReader {
    /// Constructs a new Kubernetes secrets reader.
    ///
    /// The `context` parameter works like
    /// [`KubernetesSecretsController::context`].
    pub async fn new(context: String) -> Result<KubernetesSecretsReader, anyhow::Error> {
        let (client, _) = mz_kubernetes_util::create_client(context).await?;
        let secret_api: Api<Secret> = Api::default_namespaced(client);
        Ok(KubernetesSecretsReader { secret_api })
    }
}

#[async_trait]
impl SecretsReader for KubernetesSecretsReader {
    async fn read(&self, id: GlobalId) -> Result<Vec<u8>, anyhow::Error> {
        let secret = self.secret_api.get(&secret_name(id)).await?;
        let mut data = secret
            .data
            .ok_or_else(|| anyhow!("internal error: secret missing data field"))?;
        let contents = data
            .remove("contents")
            .ok_or_else(|| anyhow!("internal error: secret missing contents field"))?;
        Ok(contents.0)
    }
}

fn secret_name(id: GlobalId) -> String {
    format!("user-managed-{id}")
}
