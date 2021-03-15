
// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::error::Error as StdError;
use std::fmt;

use tokio::io;

use crate::codec::{EncodingError, DecodingError};

#[derive(Debug)]
pub struct Error {
    inner: ErrorInner,
}

impl Error {
    pub(crate) fn connection(e: io::Error) -> Error {
        Error {
            inner: ErrorInner::Connection(e),
        }
    }
}

#[derive(Debug)]
enum ErrorInner {
    Connection(io::Error),
    Encoding(EncodingError),
    Decoding(DecodingError),
}

impl StdError for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.inner {
            ErrorInner::Connection(e) => e.fmt(f),
            ErrorInner::Encoding(e) => e.fmt(f),
            ErrorInner::Decoding(e) => e.fmt(f),
        }
    }
}

impl From<EncodingError> for Error {
    fn from(e: EncodingError) -> Error {
        Error {
            inner: ErrorInner::Encoding(e)
        }
    }
}

impl From<DecodingError> for Error {
    fn from(e: DecodingError) -> Error {
        Error {
            inner: ErrorInner::Decoding(e)
        }
    }
}