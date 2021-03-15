// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::convert::TryFrom;
use std::fmt;
use std::error::Error;
use std::io;

use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Debug)]
pub enum EncodingError {
    Io(io::Error),
    StringTooLong,
    BytesTooLong,
    ArrayTooLong,
    MessageTooLong,
    Other(String),
}

impl fmt::Display for EncodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EncodingError::Io(e) => e.fmt(f),
            EncodingError::BytesTooLong => f.write_str("bytes field too long"),
            EncodingError::StringTooLong => f.write_str("string field too long"),
            EncodingError::ArrayTooLong => f.write_str("array field too long"),
            EncodingError::MessageTooLong => f.write_str("message too long"),
            EncodingError::Other(msg) => f.write_str(msg),
        }
    }
}

impl Error for EncodingError {}

impl From<io::Error> for EncodingError {
    fn from(e: io::Error) -> EncodingError {
        EncodingError::Io(e)
    }
}

#[derive(Debug)]
pub enum DecodingError {
    Io(io::Error),
    Other(String),
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodingError::Io(e) => e.fmt(f),
            DecodingError::Other(msg) => f.write_str(msg),
        }
    }
}

impl Error for DecodingError {}

impl From<io::Error> for DecodingError {
    fn from(e: io::Error) -> DecodingError {
        DecodingError::Io(e)
    }
}

pub struct Codec;

impl<'a> Encoder<Request<'a>> for Codec {
    type Error = EncodingError;

    fn encode(&mut self, request: Request, buf: &mut BytesMut) -> Result<(), EncodingError> {
        let base = buf.len();
        request.encode(buf)?;
        let len = buf.len() - base - 4;
        let len = i32::try_from(len).map_err(|_| EncodingError::MessageTooLong)?;
        buf[base..base+4].copy_from_slice(&len.to_be_bytes());
        println!("buf is {:?}", buf);
        Ok(())
    }
}

impl<'a> Decoder for Codec {
    type Item = Response;
    type Error = DecodingError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Response>, DecodingError> {
        println!("buf is {:?}", buf);
        Ok(None)
    }
}

pub trait Encode {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError>;
}

pub trait Decode where Self: Sized {
    fn decode(buf: &mut BytesMut) -> Result<Self, DecodingError>;
}

impl Encode for bool {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        match self {
            false => buf.put_i8(0),
            true => buf.put_i8(1),
        }
        Ok(())
    }
}

impl Encode for i8 {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        buf.put_i8(*self);
        Ok(())
    }
}

impl Encode for i16 {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        buf.put_i16(*self);
        Ok(())
    }
}

impl Encode for i32 {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        buf.put_i32(*self);
        Ok(())
    }
}

impl Encode for i64 {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        buf.put_i64(*self);
        Ok(())
    }
}

impl Encode for f64 {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        buf.put_f64(*self);
        Ok(())
    }
}

impl Encode for String {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        let len = i16::try_from(self.len()).map_err(|_| EncodingError::StringTooLong)?;
        buf.put_i16(len);
        buf.put_slice(self.as_bytes());
        Ok(())
    }
}

impl Encode for Vec<u8> {
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        let len = i32::try_from(self.len()).map_err(|_| EncodingError::BytesTooLong)?;
        buf.put_i32(len);
        buf.put_slice(self);
        Ok(())
    }
}

impl<T> Encode for Vec<T>
where
    T: Encode,
{
    fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError> {
        let len = i32::try_from(self.len()).map_err(|_| EncodingError::ArrayTooLong)?;
        buf.put_i32(len);
        for item in self {
            item.encode(buf)?;
        }
        Ok(())
    }
}
