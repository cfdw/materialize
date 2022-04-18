// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::net::SocketAddr;

use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::EnvFilter;

use mz_storage2::service::storage_server::StorageServer;
use mz_storage2::service::StorageService;

/// Independent storage server for Materialize.
#[derive(clap::Parser)]
struct Args {
    /// The address on which to listen for connections.
    #[clap(
        long,
        env = "STORAGED_LISTEN_ADDR",
        value_name = "HOST:PORT",
        default_value = "127.0.0.1:2100"
    )]
    listen_addr: SocketAddr,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(mz_ore::cli::parse_args()).await {
        eprintln!("storaged: fatal: {}", e);
    }
}

async fn run(args: Args) -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("STORAGED_LOG_FILTER")
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Listening on {}...", args.listen_addr);
    Server::builder()
        .add_service(StorageServer::new(StorageService))
        .serve(args.listen_addr)
        .await?;
    Ok(())
}
