// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;

use mz_persist_client::ShardId;

/// An external source of data.
pub trait Source: Sized {
    /// The key type for messages produced by the source.
    ///
    /// Use `()` if messages do not have keys.
    type Key;

    /// The value type for messages produced by the source.
    type Value;

    /// The type that identifies a partition in the source.
    ///
    /// Use `()` if the source does not have the concept of a partition.
    type Partition;

    /// The reader for the source.
    type Reader: SourceReader<Self>;

    // The offset fetcher for the source.
    type OffsetFetcher: SourceOffsetFetcher<Self>;

    /// Constructs a reader for the source that starts at the specified offsets
    /// for each partition.
    fn reader(&self, start_offsets: BTreeMap<Self::Partition, Offset>) -> Self::Reader;

    /// Fetches the latest offsets for each partition of the source.
    // TODO(benesch): in the future we may want to get more clever for sources
    // that don't have an efficient way of fetching the latest offset. To start,
    // maybe let's try reading it twice and seeing how bad that feels?
    async fn fetch_latest_offsets(&self) -> BTreeMap<S::Partition, Offset>;
}

/// Reads messages from a [`Source`].
#[async_trait]
pub trait SourceReader<S>
where
    S: Source,
{
    /// Fetches the next message.
    async fn next_message(&mut self) -> Message<S>;
}

/// The offset of a [`Message`].
///
/// The only constraint on an offset is that it monotonically increases within
/// a shard.
pub struct Offset(u64);

/// A message produced by a [`Source`].
pub struct Message<S>
where
    S: Source,
{
    /// The message's key.
    key: S::Key,
    /// The message's value.
    value: S::Value,
    /// The message's offset within its partition.
    offset: Offset,
    /// The message's partition.
    partition: S::Partition,
}

/// Configures a source.
pub struct SourceConfig {
    /// The persist client.
    pub persist_client: mz_persist_client::Client,
    /// The ID of the shard where the source's data is stored.
    pub data_shard: ShardId,
    /// The ID of the shard where the source's timestamp bindings are stored.
    pub timestamp_binding_shard: ShardId,
    /// The interval at which to emit timestamp bindings.
    pub timestamp_interval: Duration,
}

async fn start<S>(source: &S, config: &SourceConfig) -> Result<(), anyhow::Error>
where
    S: Source,
{
    // Find the last offset read from each partition.
    let (binding_write, binding_read) = config
        .persist_client
        .open(Duration::MAX, config.timestamp_binding_shard)
        .await?;
    let binding_upper = binding_write.upper();
    // Do more stuff with binding_read to get the latest timestamp bindings.

    let (data_write, data_read) = config
        .persist_client
        .open(Duration::MAX, config.timestamp_binding_shard)
        .await?;
    let data_upper = data_write.upper();
    // Correlate the data upper with the timestamp bindings to figure out
    // what offset to begin reading at.

    tokio::spawn(async {
        let mut interval = time::interval(config.timestamp_interval);
        loop {
            interval.tick().await;
            let offsets = source.fetch_latest_offsets().await;
            // TODO: binding_write.append(...)
        }
    })

    tokio::spawn({
        let start_offsets = BTreeMap::new(); // TODO: compute this from the bindings.
        let reader = source.reader(start_offsets);
        async move {
            let mut binding_listener = binding_read.listen();

            while let Ok(binding_listener) = binding_listener.poll_next(Duration::MAX) {
                // TODO: figure out how many new offsets to read.

                while let Some(message) = reader.next().await {
                    // TODO: decode, maybe in parallel?
                    // TODO: break when we've read the right number of offsets.
                    // TODO: write to persist.
                }
            }
        }
    });

    Ok(())
}
