// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::convert::TryFrom;

use bytes::{BufMut, BytesMut};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, ToSocketAddrs};

use crate::codec::EncodingError;
use crate::error::Error;
use crate::messages::Request;

pub struct Conn {
    inner: TcpStream,
    write_buf: BytesMut,
}

impl Conn {
    pub async fn connect<A>(addr: A) -> Result<Conn, Error>
    where
        A: ToSocketAddrs,
    {
        let inner = TcpStream::connect(addr).await.map_err(Error::connection)?;
        Ok(Conn {
            inner,
            write_buf: BytesMut::new(),
        })
    }

    pub async fn send<R>(&mut self, r: R) -> Result<R::Response, Error>
    where
        R: Request,
    {
        self.write_buf.clear();
        self.write_buf.put_i32(0);
        r.encode(&mut self.write_buf);
        let len = i32::try_from(self.write_buf.len()).map_err(|_| EncodingError::MessageTooLong)?;
        self.write_buf[..4].copy_from_slice(&len.to_be_bytes());
        self.inner.write_all(&self.write_buf).await?;
        Ok(())
    }
}
