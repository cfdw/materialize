# Connectors

## Summary

This document proposes a design for **connectors**, a new type of catalog object
that allows common configuration parameters to be shared across sources and
sinks.

## Overview

Many users of Materialize create families of sources and sinks that share many
configuration parameters. The current design of `CREATE SOURCE` and `CREATE
SINK` make this a verbose affair. The problem is particularly acute with
Avro-formatted Kafka sources that configure authentication:

```sql
CREATE SOURCE kafka1
FROM KAFKA BROKER 'kafka:9092' TOPIC 'top1' WITH (
    security_protocol = 'SASL_SSL',
    sasl_mechanisms = 'PLAIN',
    sasl_username = 'username',
    sasl_password = 'password',
)
FORMAT AVRO USING CONFLUENT SCHEMA REGISTRY 'https://schema-registry' WITH (
    username = 'username',
    password = 'password'
);

CREATE SOURCE kafka2
FROM KAFKA BROKER 'kafka:9092' TOPIC 'top1' WITH (
    security_protocol = 'SASL_SSL',
    sasl_mechanisms = 'PLAIN',
    sasl_username = 'username',
    sasl_password = 'password'
)
FORMAT AVRO USING CONFLUENT SCHEMA REGISTRY 'https://schema-registry' WITH (
    username = 'username',
    password = 'password'
);
```

These two source definition differ only in their topic specification, but
must duplicate eight other configuration parameters.

With the connectors proposed in this document, the `CREATE SOURCE` can instead
share all the relevant configuration:

```sql
CREATE CONNECTOR kafka FOR
KAFKA BROKER 'kafka:9092' WITH (
    security_protocol = 'SASL_SSL',
    sasl_mechanisms = 'PLAIN',
    sasl_username = 'username',
    sasl_password = 'password'
);

CREATE CONNECTOR schema_registry FOR
CONFLUENT SCHEMA REGISTRY 'https://schema-registry' WITH (
    username = 'username',
    password = 'password'
);

CREATE SOURCE kafka1
FROM KAFKA CONNECTOR kafka
FORMAT AVRO USING CONFLUENT SCHEMA REGISTRY CONNECTOR schema_registry;

CREATE SOURCE kafka2
FROM KAFKA CONNECTOR kafka
FORMAT AVRO USING CONFLUENT SCHEMA REGISTRY CONNECTOR schema_registry;
```

## Design

### SQL syntax and semantics


## Reference

### Kafka connector
### Confluent Schema Registry connector
### PostgreSQL connector
### AWS connector
