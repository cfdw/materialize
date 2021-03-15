// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use tokio::net::ToSocketAddrs;

use crate::error::Error;
use crate::conn::Conn;

pub struct Client {
    conn: Conn,
}

impl Client {
    pub async fn connect<A>(addr: A) -> Result<Client, Error>
    where
        A: ToSocketAddrs,
    {
        let conn = Conn::connect(addr).await?;
        Ok(Client {
            conn
        })
    }
}

