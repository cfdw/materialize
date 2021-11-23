// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use dataflow_types::{DecodeError, ProtobufEncoding};
use interchange::protobuf::{DecodedDescriptors, Decoder};
use repr::Row;

#[derive(Debug)]
pub struct ProtobufDecoderState {
    decoder: Decoder,
    confluent_wire_format: bool,
    events_success: i64,
    events_error: i64,
}

impl ProtobufDecoderState {
    pub fn new(
        ProtobufEncoding {
            descriptors,
            message_name,
            confluent_wire_format,
        }: ProtobufEncoding,
    ) -> Self {
        let descriptors = DecodedDescriptors::from_bytes(&descriptors, message_name)
            .expect("descriptors provided to protobuf source are pre-validated");
        ProtobufDecoderState {
            decoder: Decoder::new(descriptors),
            confluent_wire_format,
            events_success: 0,
            events_error: 0,
        }
    }
    pub fn get_value(&mut self, mut bytes: &[u8]) -> Option<Result<Row, DecodeError>> {
        if self.confluent_wire_format {
            // The first byte is a magic byte (0) that indicates the Confluent
            // serialization format version, and the next four bytes are a big
            // endian 32-bit schema ID.
            //
            // https://docs.confluent.io/current/schema-registry/docs/serializer-formatter.html#wire-format
            if bytes.len() < 5 {
                return Some(Err(DecodeError::Text(format!(
                    "Confluent-style Protobuf datum is too few bytes: expected at least 5 bytes, got {}",
                    bytes.len()
                ))));
            }
            // For Protobuf, we just ignore the schema ID in the Confluent
            // header. Unlike Avro, there's no concept of "resolving" a
            // Protobuf reader schema against a writer schema. Instead we just
            // have to trust that whoever wrote the data did so with a schema
            // that is compatible with ours.
            bytes = &bytes[5..];
        }
        match self.decoder.decode(bytes) {
            Ok(row) => {
                if let Some(row) = row {
                    self.events_success += 1;
                    Some(Ok(row))
                } else {
                    self.events_error += 1;
                    Some(Err(DecodeError::Text(format!(
                        "protobuf deserialization returned None"
                    ))))
                }
            }
            Err(err) => {
                self.events_error += 1;
                Some(Err(DecodeError::Text(format!(
                    "protobuf deserialization error: {:#}",
                    err
                ))))
            }
        }
    }
}
