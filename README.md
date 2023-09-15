<!-- markdownlint-configure-file
{
    "no-duplicate-header": {
        "siblings_only": true
    }
}
-->

# influxdb-compute-api

[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://conventionalcommits.org)

## Description

This service offers an HTTP API that serves data computed from InfluxDB.

## Endpoints

### Health

#### `GET` `/health`

Returns the service health status.

#### Parameters

None

#### Response

| Code | Description          |
| ---- | -------------------- |
| 204  | Service is healthy   |
| 500  | Service is unhealthy |

### Performance ratio

#### `GET` `/performance/{id}`

Returns the performance ratio.

#### Parameters

| Name                         | Description                                 |
| ---------------------------- | ------------------------------------------- |
| `id` _(path)_                | Partner ID                                  |
| `clientTime` _(query param)_ | Client request timestamp ([RFC3339] format) |

[RFC3339]: https://datatracker.ietf.org/doc/html/rfc3339

#### Response

| Code | Description                        |
| ---- | ---------------------------------- |
| 200  | Performance ratio as a JSON number |
| 400  | Bad request                        |
| 500  | Internal error                     |

### Timeline

#### `POST` `/timeline/{id}`

Returns the timeline data.

#### Parameters

| Name        | Description |
| ----------- | ----------- |
| id _(path)_ | Partner ID  |

#### Response

| Code | Description                                                  |
| ---- | ------------------------------------------------------------ |
| 200  | Timeline data ([MessagePack][msgpack] format), __see below__ |
| 500  | Internal error                                               |

Timeline data consists of an array of arrays. Inner arrays contain following components:

1. Slot start date and time in seconds since epoch (integer);
2. Index of the color in an abstract palette (integer).

[msgpack]: https://msgpack.org/

## Usage

```ShellSession
$ influxdb-compute-api --help
Usage: influxdb-compute-api [OPTIONS] --config-api-url <CONFIG_API_URL> --influxdb-api-token <INFLUXDB_API_TOKEN> --influxdb-org <INFLUXDB_ORG> --influxdb-bucket <INFLUXDB_BUCKET> --influxdb-measurement <INFLUXDB_MEASUREMENT>

Options:
      --listen-address <LISTEN_ADDRESS>
          Address to listen on [env: LISTEN_ADDRESS=] [default: 0.0.0.0:8080]
      --config-api-url <CONFIG_API_URL>
          Configuration API URL [env: CONFIG_API_URL=]
      --common-config-cache-expiration <COMMON_CONFIG_CACHE_EXPIRATION>
          Expiration time for common configuration cache [env: COMMON_CONFIG_CACHE_EXPIRATION=] [default: 1m]
      --influxdb-url <INFLUXDB_URL>
          InfluxDB base URL [env: INFLUXDB_URL=] [default: http://influxdb:8086]
      --influxdb-api-token <INFLUXDB_API_TOKEN>
          InfluxDB API token with read permission on configured bucket [env: INFLUXDB_API_TOKEN=]
      --influxdb-org <INFLUXDB_ORG>
          InfluxDB organization name or ID [env: INFLUXDB_ORG=]
      --influxdb-bucket <INFLUXDB_BUCKET>
          InfluxDB bucket [env: INFLUXDB_BUCKET=]
      --influxdb-measurement <INFLUXDB_MEASUREMENT>
          InfluxDB measurement [env: INFLUXDB_MEASUREMENT=]
  -v, --verbose...
          More output per occurrence
  -q, --quiet...
          Less output per occurrence
  -h, --help
          Print help
```
