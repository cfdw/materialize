// Copyright Materialize, Inc. All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License in the LICENSE file at the
// root of this repository, or online at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::VecDeque;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::ops::RangeInclusive;
use std::path::PathBuf;

use anyhow::Context;
use fstrings::{f, format_args_f};
use json_comments::StripComments;
use serde::{Deserialize, Deserializer};
use serde::de::Error;

use ore::codegen::CodegenBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    api_key: Option<i16>,
    #[serde(rename = "type")]
    ty: MessageType,
    name: String,
    valid_versions: VersionRangeInclusive,
    flexible_versions: Option<String>,
    fields: Vec<MessageField>,
    common_structs: Option<Vec<MessageCommonStruct>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
enum MessageType {
    #[serde(rename = "header")]
    Header,
    #[serde(rename = "request")]
    Request,
    #[serde(rename = "response")]
    Response,
}

#[derive(Debug, Deserialize)]
struct MessageField {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    versions: String,
    ignorable: Option<bool>,
    about: Option<String>,
    fields: Option<Vec<MessageField>>,
}

#[derive(Debug, Deserialize)]
struct MessageCommonStruct {
    name: String,
    fields: Vec<MessageField>,
}

#[derive(Debug)]
struct RecordType<'a> {
    name: &'a str,
    prefix: &'a str,
    fields: &'a [MessageField],
}

#[derive(Debug)]
struct VersionRangeInclusive(RangeInclusive<usize>);

impl<'de> Deserialize<'de> for VersionRangeInclusive {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        let mut pieces = s.splitn(2, "-");
        let lb: usize = pieces.next().unwrap().parse().map_err(|e| {
            D::Error::custom(format!("unable to parse integer in version range: {}", e))
        })?;
        match pieces.next() {
            None => Ok(VersionRangeInclusive(lb..=lb)),
            Some(ub) => {
                let ub = ub.parse().map_err(|e| {
                    D::Error::custom(format!("unable to parse integer in version range: {}", e))
                })?;
                Ok(VersionRangeInclusive(lb..=ub))
            }
        }
    }
}

impl<'a> IntoIterator for &'a VersionRangeInclusive {
    type IntoIter = RangeInclusive<usize>;
    type Item = usize;

    fn into_iter(self) -> Self::IntoIter {
        self.0.clone()
    }
}

fn main() -> Result<(), anyhow::Error> {
    let messages = {
        let mut messages: Vec<Message> = vec![];
        let dir = fs::read_dir("resources/messages").context("reading message definitions")?;
        for entry in dir {
            let entry = entry.context("reading directory entry")?;
            if entry.path().extension() != Some(OsStr::new("json")) {
                continue;
            }
            let name = PathBuf::from(entry.file_name());
            let file = File::open(entry.path())
                .with_context(|| format!("opening message definition {}", name.display()))?;
            let message = serde_json::from_reader(StripComments::new(file))
                .with_context(|| format!("reading message definition {}", name.display()))?;
            messages.push(message);
        }
        messages.sort_by_key(|m| (m.api_key.unwrap_or(-1), m.ty));
        messages
    };

    let mut buf = CodegenBuf::new();
    buf.writeln("use bytes::BytesMut;");
    buf.writeln("use crate::codec::{Encode, Decode, EncodingError, DecodingError};");

    for message in &messages {
        for version in &message.valid_versions {
            gen_message(&mut buf, version, message)
        }
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").context("Cannot read OUT_DIR env var")?);
    fs::write(out_dir.join("messages.rs"), buf.into_string())?;

    Ok(())
}

fn gen_message(buf: &mut CodegenBuf, version: usize, message: &Message) {
    fn gen_type<'a>(prefix: &'a str, ty: &'a str, fields: Option<&'a [MessageField]>, queue: &mut VecDeque<RecordType<'a>>) -> String {
        match ty {
            "bool" => "bool".into(),
            "int8" => "i8".into(),
            "int16" => "i16".into(),
            "int32" => "i32".into(),
            "int64" => "i64".into(),
            "float64" => "f64".into(),
            "string" => "String".into(),
            "bytes" => "Vec<u8>".into(),
            other => {
                if let Some(ty) = other.strip_prefix("[]") {
                    let ty = gen_type(prefix, ty, fields, queue);
                    f!("Vec<{ty}>")
                } else {
                    if let Some(fields) = fields {
                        queue.push_back(RecordType {
                            name: &ty,
                            prefix,
                            fields,
                        });
                    }
                    f!("{prefix}{other}")
                }
            }
        }
    }

    let prefix = &f!("{message.name}V{version}");
    let mut queue = VecDeque::new();
    queue.push_back(RecordType {
        name: &prefix,
        prefix: "",
        fields: &message.fields,
    });
    if let Some(common_structs) = &message.common_structs {
        for cs in common_structs {
            queue.push_back(RecordType {
                name: &cs.name,
                prefix: &prefix,
                fields: &cs.fields,
            });
        }
    }
    while let Some(record) = queue.pop_front() {
        let name = f!("{record.prefix}{record.name}");
        buf.start_block(f!("struct {name}"));
        for field in record.fields {
            let name = to_snake_case(&field.name);
            let ty = gen_type(&prefix, &field.ty, field.fields.as_deref(), &mut queue);
            buf.writeln(f!("{name}: {ty},"));
        }
        buf.end_block();

        buf.start_block(f!("impl Encode for {name}"));
        buf.start_block(f!("fn encode(&self, buf: &mut BytesMut) -> Result<(), EncodingError>"));
        for field in record.fields {
            let name = to_snake_case(&field.name);
            buf.writeln(f!("self.{name}.encode(buf)?;"));
        }
        buf.writeln("Ok(())");
        buf.end_block();
        buf.end_block();
    }
}

fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    if out == "match" {
        "match_".into()
    } else {
        out
    }
}
