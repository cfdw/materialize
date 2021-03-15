// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Jackdaw is a high-performance, low-level client for Apache Kafka.

mod client;
mod codec;
mod conn;
mod error;
mod messages;

pub use client::Client;
