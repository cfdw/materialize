// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Durable metadata storage.

use std::cmp;
use std::error::Error;
use std::fmt;
use std::iter;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, Mutex};

use persist_types::Codec;
use rusqlite::{named_params, params, Connection, Transaction};

const APPLICATION_ID: i32 = 0x0872_e898; // chosen randomly

const SCHEMA: &str = "
CREATE TABLE arrangements (
    arrangement_id integer PRIMARY KEY,
    name text NOT NULL UNIQUE
);

CREATE TABLE data (
    arrangement_id integer NOT NULL REFERENCES arrangements (arrangement_id),
    key blob NOT NULL,
    value blob NOT NULL,
    time integer NOT NULL,
    diff integer NOT NULL,
    UNIQUE (arrangement_id, key, value, time)
);

CREATE INDEX data_time_idx ON data (arrangement_id, time);

CREATE TABLE sinces (
    arrangement_id NOT NULL UNIQUE REFERENCES arrangements (arrangement_id),
    since integer NOT NULL
);

CREATE TABLE uppers (
    arrangement_id NOT NULL UNIQUE REFERENCES arrangements (arrangement_id),
    upper integer NOT NULL
);
";

/// A durable metadata store.
///
/// A stash manages any number of named [`StashArrangement`]s.
///
/// Data is stored in a single file on disk. The format of this file is not
/// specified and should not be relied upon. The only promise is stability. Any
/// changes to the on-disk format will be accompanied by a clear migration path.
///
/// A stash is designed to store only a small quantity of data. Think megabytes,
/// not gigabytes.
///
/// The API of a stash intentionally mimics the API of a [STORAGE] collection.
/// You can think of stash as a stable but very low performance STORAGE
/// collection. When the STORAGE layer is stable enough to serve as a source of
/// truth, the intent is to swap all stashes for STORAGE collections.
///
/// [STORAGE]: https://github.com/MaterializeInc/materialize/blob/main/doc/developer/platform/architecture-db.md#STORAGE
pub struct Stash {
    conn: Arc<Mutex<Connection>>,
}

impl Stash {
    /// Opens the stash stored at the specified path.
    pub fn open(path: &Path) -> Result<Stash, StashError> {
        let mut conn = Connection::open(path)?;
        let tx = conn.transaction()?;
        let app_id: i32 = tx.query_row("PRAGMA application_id", params![], |row| row.get(0))?;
        if app_id == 0 {
            tx.execute_batch(&format!(
                "PRAGMA application_id = {APPLICATION_ID};
                 PRAGMA user_version = 1;"
            ))?;
            tx.execute_batch(SCHEMA)?;
        } else if app_id != APPLICATION_ID {
            return Err(StashError::from(format!(
                "invalid application id: {}",
                app_id
            )));
        }
        tx.commit()?;
        Ok(Stash {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Loads or creates the named arrangement.
    ///
    /// If the arrangement with the specified name does not yet exist, it is
    /// created with no entries, a zero since frontier, and a zero upper
    /// frontier. Otherwise the existing durable state is loaded.
    ///
    /// It is the callers responsibility to keep `K` and `V` fixed for a given
    /// arrangement in a given stash for the lifetime of the stash.
    ///
    /// It is valid to construct multiple handles to the same named arrangement
    /// and use them simultaneously.
    pub fn arrangement<K, V>(&self, name: &str) -> Result<StashArrangement<K, V>, StashError>
    where
        K: Codec + Ord,
        V: Codec + Ord,
    {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO arrangements (name) VALUES ($name) ON CONFLICT DO NOTHING",
            named_params! {"$name": name},
        )?;
        let arrangement_id = tx.query_row(
            "SELECT arrangement_id FROM arrangements WHERE name = $name",
            named_params! {"$name": name},
            |row| row.get("arrangement_id"),
        )?;
        tx.execute(
            "INSERT INTO sinces (arrangement_id, since) VALUES ($arrangement_id, $since)
             ON CONFLICT DO NOTHING",
            named_params! {"$arrangement_id": arrangement_id, "$since": 0_i64},
        )?;
        tx.execute(
            "INSERT INTO uppers (arrangement_id, upper) VALUES ($arrangement_id, $upper)
             ON CONFLICT DO NOTHING",
            named_params! {"$arrangement_id": arrangement_id, "$upper": 0_i64},
        )?;
        tx.commit()?;
        Ok(StashArrangement {
            conn: self.conn.clone(),
            arrangement_id,
            _kv: PhantomData,
        })
    }
}

/// `StashArrangement` is analogous to a [differential dataflow arrangement],
/// but the state of the collection is durable.
///
/// A `StashArrangement` stores `(key, value, timestamp, diff)` entries. The key
/// and value types are chosen by the caller; they must implement [`Ord`] and
/// they must be serializable to and deserializable from bytes via the [`Codec`]
/// trait. The timestamp and diff types are fixed to `i64`.
///
/// A `StashArrangement` maintains a since frontier and an upper frontier, as
/// described in the [correctness vocabulary document]. To advance the since
/// frontier, call [`compact`]. To advance the upper frontier, call [`seal`]. To
/// physically compact data beneath the since frontier, call [`consolidate`].
///
/// [`compact`]: StashArrangement::compact
/// [`consolidate`]: StashArrangement::consolidate
/// [`seal`]: StashArrangement::seal
/// [correctness vocabulary document]:
///     https://github.com/MaterializeInc/materialize/blob/main/doc/developer/design/20210831_correctness.md
/// [differential dataflow arrangement]:
///     differential_dataflow::operators::arrange::arrangement
pub struct StashArrangement<K, V>
where
    K: Codec + Ord,
    V: Codec + Ord,
{
    conn: Arc<Mutex<Connection>>,
    arrangement_id: i64,
    _kv: PhantomData<(K, V)>,
}

impl<K, V> StashArrangement<K, V>
where
    K: Codec + Ord,
    V: Codec + Ord,
{
    /// Iterates over all entries in the stash.
    ///
    /// Entries are iterated in `(key, value, time)` order and are guaranteed
    /// to be consolidated.
    ///
    /// Each entry's time is guaranteed to be greater than or equal to the since
    /// frontier. The time may also be greater than the upper frontier,
    /// indicating data that has not yet been made definite.
    ///
    /// [`consolidate`]: StashArrangement::consolidate
    /// [`update`]: StashArrangement::update
    /// [`update_many`]: StashArrangement::update_many
    pub fn iter(&self) -> Result<impl Iterator<Item = ((K, V), i64, i64)>, StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let since = self.since_tx(&tx)?;
        let mut rows = tx
            .prepare(
                "SELECT key, value, time, diff FROM data
                 WHERE arrangement_id = $arrangement_id",
            )?
            .query_and_then(
                named_params! {"$arrangement_id": self.arrangement_id},
                |row| {
                    let key_buf: Vec<_> = row.get("key")?;
                    let value_buf: Vec<_> = row.get("value")?;
                    let key = K::decode(&key_buf)?;
                    let value = V::decode(&value_buf)?;
                    let time = row.get("time")?;
                    let diff = row.get("diff")?;
                    Ok::<_, StashError>(((key, value), cmp::max(time, since), diff))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        differential_dataflow::consolidation::consolidate_updates(&mut rows);
        Ok(rows.into_iter())
    }

    /// Iterates over entries in the stash for the given key.
    ///
    /// Entries are iterated in `(value, timestamp)` order and are guaranteed
    /// to be consolidated.
    ///
    /// Each entry's time is guaranteed to be greater than or equal to the since
    /// frontier. The time may also be greater than the upper frontier,
    /// indicating data that has not yet been made definite.
    ///
    /// [`consolidate`]: StashArrangement::consolidate
    /// [`update`]: StashArrangement::update
    /// [`update_many`]: StashArrangement::update_many
    pub fn iter_key(&self, key: K) -> Result<impl Iterator<Item = (V, i64, i64)>, StashError> {
        let mut key_buf = vec![];
        key.encode(&mut key_buf);
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let since = self.since_tx(&tx)?;
        let mut rows = tx
            .prepare(
                "SELECT value, time, diff FROM data
                 WHERE arrangement_id = $arrangement_id AND key = $key",
            )?
            .query_and_then(
                named_params! {
                    "$arrangement_id": self.arrangement_id,
                    "$key": key_buf,
                },
                |row| {
                    let value_buf: Vec<_> = row.get("value")?;
                    let value = V::decode(&value_buf)?;
                    let time = row.get("time")?;
                    let diff = row.get("diff")?;
                    Ok::<_, StashError>((value, cmp::max(time, since), diff))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        differential_dataflow::consolidation::consolidate_updates(&mut rows);
        Ok(rows.into_iter())
    }

    /// Adds a single entry to the arrangement.
    ///
    /// The entry's time must be greater than or equal to the upper frontier.
    ///
    /// If this method returns `Ok`, the entry has been made durable.
    pub fn update(&mut self, data: (K, V), time: i64, diff: i64) -> Result<(), StashError> {
        self.update_many(iter::once((data, time, diff)))
    }

    /// Atomically adds multiple entries to the arrangement.
    ///
    /// Each entry's time must be greater than or equal to the upper frontier.
    ///
    /// If this method returns `Ok`, the entries have been made durable.
    pub fn update_many<I>(&mut self, entries: I) -> Result<(), StashError>
    where
        I: IntoIterator<Item = ((K, V), i64, i64)>,
    {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let upper = self.upper_tx(&tx)?;
        let mut insert_stmt = tx.prepare(
            "INSERT INTO data (arrangement_id, key, value, time, diff)
             VALUES ($arrangement_id, $key, $value, $time, $diff)
             ON CONFLICT (arrangement_id, key, value, time) DO UPDATE SET diff = diff + excluded.diff",
        )?;
        let mut delete_stmt = tx.prepare(
            "DELETE FROM data
             WHERE arrangement_id = $arrangement_id AND key = $key AND value = $value AND time = $time AND diff = 0",
        )?;
        let mut key_buf = vec![];
        let mut value_buf = vec![];
        for ((key, value), time, diff) in entries {
            assert!(upper <= time);
            key_buf.clear();
            value_buf.clear();
            key.encode(&mut key_buf);
            value.encode(&mut value_buf);
            insert_stmt.execute(named_params! {
                "$arrangement_id": self.arrangement_id,
                "$key": key_buf,
                "$value": value_buf,
                "$time": time,
                "$diff": diff,
            })?;
            delete_stmt.execute(named_params! {
                "$arrangement_id": self.arrangement_id,
                "$key": key_buf,
                "$value": value_buf,
                "$time": time,
            })?;
        }
        drop(insert_stmt);
        drop(delete_stmt);
        tx.commit()?;
        Ok(())
    }

    /// Advances the upper frontier to the specified value.
    ///
    /// The provided `upper` must be greater than or equal to the current upper
    /// frontier.
    ///
    /// Intuitively, this method declares that all times less than `upper` are
    /// definite.
    pub fn seal(&self, upper: i64) -> Result<(), StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        assert!(self.upper_tx(&tx)? <= upper);
        tx.execute(
            "UPDATE uppers SET upper = $upper WHERE arrangement_id = $arrangement_id",
            named_params! {"$upper": upper, "$arrangement_id": self.arrangement_id},
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Advances the since frontier to the specified value.
    ///
    /// The provided `since` must be greater than or equal to the current since
    /// frontier but less than or equal to the current upper frontier.
    ///
    /// Intuitively, this method performs logical compaction. Existing entries
    /// whose time is less than `since` are fast-forwarded to `since`.
    pub fn compact(&self, since: i64) -> Result<(), StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        assert!(since <= self.upper_tx(&tx)?);
        assert!(self.since_tx(&tx)? <= since);
        tx.execute(
            "UPDATE sinces SET since = $since WHERE arrangement_id = $arrangement_id",
            named_params! {"$since": since, "$arrangement_id": self.arrangement_id},
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Consolidates entries less than the since frontier.
    ///
    /// Intuitively, this method performs physical compaction. Existing
    /// key–value pairs whose time is less than the since frontier are
    /// consolidated together when possible.
    pub fn consolidate(&mut self) -> Result<(), StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let since = self.since_tx(&tx)?;
        tx.execute(
            "INSERT INTO data (arrangement_id, key, value, time, diff)
             SELECT arrangement_id, key, value, $since, sum(diff) FROM data
             WHERE arrangement_id = $arrangement_id AND time < $since
             GROUP BY key, value
             ON CONFLICT (arrangement_id, key, value, time) DO UPDATE SET diff = diff + excluded.diff",
            named_params! {
                "$arrangement_id": self.arrangement_id,
                "$since": since,
            },
        )?;
        tx.execute(
            "DELETE FROM data WHERE arrangement_id = $arrangement_id AND time < $since",
            named_params! {
                "$arrangement_id": self.arrangement_id,
                "$since": since,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Reports the current since frontier.
    pub fn since(&self) -> Result<i64, StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let since = self.since_tx(&tx)?;
        tx.commit()?;
        Ok(since)
    }

    /// Reports the current upper frontier.
    pub fn upper(&self) -> Result<i64, StashError> {
        let mut conn = self.conn.lock().expect("lock poisoned");
        let tx = conn.transaction()?;
        let upper = self.upper_tx(&tx)?;
        tx.commit()?;
        Ok(upper)
    }

    fn since_tx(&self, tx: &Transaction) -> Result<i64, StashError> {
        let upper = tx.query_row(
            "SELECT since FROM sinces WHERE arrangement_id = $arrangement_id",
            named_params! {"$arrangement_id": self.arrangement_id},
            |row| row.get("since"),
        )?;
        Ok(upper)
    }

    fn upper_tx(&self, tx: &Transaction) -> Result<i64, StashError> {
        let upper = tx.query_row(
            "SELECT upper FROM uppers WHERE arrangement_id = $arrangement_id",
            named_params! {"$arrangement_id": self.arrangement_id},
            |row| row.get("upper"),
        )?;
        Ok(upper)
    }
}

/// An error that can occur while interacting with a [`Stash`].
///
/// Stash errors are deliberately opaque. They generally indicate unrecoverable
/// conditions, like running out of disk space.
#[derive(Debug)]
pub struct StashError {
    // Internal to avoid leaking implementation details about SQLite.
    inner: InternalStashError,
}

#[derive(Debug)]
enum InternalStashError {
    Sqlite(rusqlite::Error),
    Other(String),
}

impl fmt::Display for StashError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("stash error: ")?;
        match &self.inner {
            InternalStashError::Sqlite(e) => e.fmt(f),
            InternalStashError::Other(e) => f.write_str(&e),
        }
    }
}

impl Error for StashError {}

impl From<rusqlite::Error> for StashError {
    fn from(e: rusqlite::Error) -> StashError {
        StashError {
            inner: InternalStashError::Sqlite(e),
        }
    }
}

impl From<String> for StashError {
    fn from(e: String) -> StashError {
        StashError {
            inner: InternalStashError::Other(e),
        }
    }
}
