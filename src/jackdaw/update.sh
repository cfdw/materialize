#!/usr/bin/env bash

# Copyright Materialize, Inc. All rights reserved.
#
# Use of this software is governed by the Business Source License
# included in the LICENSE file at the root of this repository.
#
# As of the Change Date specified in that file, in accordance with
# the Business Source License, use of this software will be governed
# by the Apache License, Version 2.0.

set -euo pipefail

cd "$(dirname "$0")"

. ../../misc/shlib/shlib.bash

if [[ ! "${1:-}" ]]; then
    die "usage: $0 KAFKA-VERSION"
fi

rm -rf resources/messages
mkdir -p resources/messages

curl -fsSL "https://github.com/apache/kafka/archive/$1.tar.gz" \
  | tar xz -C resources/messages "kafka-$1/clients/src/main/resources/common/message" --strip-components=7

cat > resources/messages/README.md <<'EOF'
# Apache Kafka protocol definitions

These files are automatically imported from the Apache Kafka repository by the
`update.sh` script in the root of this crate.
EOF
