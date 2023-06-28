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

1. Slot start date and time in seconds since epoch (32-bit unsigned integer);
2. Slot end date and time in seconds since epoch (32-bit unsigned integer);
3. Slot color as a 3-tuple of 8-bit values, representing red, green and blue components.

[msgpack]: https://msgpack.org/

## Usage

```ShellSession
$ influxdb-compute-api --help
Usage: 🚧 WIP 🚧
```
