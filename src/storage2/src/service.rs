// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::pin::Pin;

use async_stream::stream;
use futures::stream::Stream;
use tonic::{async_trait, Code, Request, Response, Status, Streaming};

use crate::service::storage_server::Storage;

tonic::include_proto!("mz_storage2.service");

pub struct StorageService;

#[async_trait]
impl Storage for StorageService {
    type ControlStream = Pin<Box<dyn Stream<Item = Result<StorageResponse, Status>> + Send>>;

    async fn control(
        &self,
        request: Request<Streaming<StorageCommand>>,
    ) -> Result<Response<Self::ControlStream>, Status> {
        Ok(Response::new(Box::pin(stream! {
            for await msg in request.into_inner() {
                println!("msg is {msg:?}");
            }
            yield Err(Status::new(Code::InvalidArgument, "TODO"))
        })))
    }
}
