#!/usr/bin/env bash

me="$0"
log_file=

teardown () {
    if [ "$log_file" ]; then
        docker compose stop
        docker compose logs --timestamps > "$log_file"
    fi
    docker compose down --volumes
}

die () {
    echo "$1" >&2
    teardown
    exit 1
}

while :; do
    case $1 in
        -h|--help)
            echo "Usage: $me [--log-file path]"
            exit 2
            ;;
        --log-file)
            if [ "$2" ]; then
                if touch "$2"; then
                    log_file=$2
                    shift
                else
                    die "log file error"
                fi
            else
                die '"--log-file" requires a non-empty option argument'
            fi
            ;;
        *)
            break
    esac
done

set -eux

# Build services images
docker compose build

# Start services
docker compose up -d --quiet-pull influxdb influxdb-compute-api

# Wait for config API to be ready
docker compose up -d --quiet-pull config-api
max_attempts=5
wait_success=
for i in $(seq 1 $max_attempts); do
    if docker compose exec config-api deno run --allow-net check.ts; then
        wait_success="true"
        break
    fi
    echo "Waiting for config API to be healthy: try #$i failed" >&2
    [[ $i != "$max_attempts" ]] && sleep 3
done
if [ "$wait_success" != "true" ]; then
    die "Failure waiting for config API to be healthy"
fi

# Wait for service to be healthy
max_attempts=4
service="influxdb-compute-api"
wait_success=
for i in $(seq 1 $max_attempts); do
    if docker compose exec $service /usr/local/bin/healthcheck; then
        wait_success="true"
        break
    fi
    echo "$me: waiting for $service to be healthy: try #$i failed" >&2
    [[ $i != "$max_attempts" ]] && sleep 5
done
if [ "$wait_success" != "true" ]; then
    die "failure waiting for $service to be healthy"
fi

# Feed InfluxDB with timeline data
awk '{ $NF=systime()+$NF; print }' timeline_data.txt | \
    docker compose exec -T influxdb sh -c "cat - > /usr/src/timeline_data.txt"
docker compose exec influxdb \
    influx write \
    --bucket integration_tests_bucket \
    --precision s \
    --file /usr/src/timeline_data.txt

# Feed InfluxDB with performance data
awk '{ $NF=systime()+$NF; print }' performance_data.txt | \
    docker compose exec -T influxdb sh -c "cat - > /usr/src/performance_data.txt"
docker compose exec influxdb \
    influx write \
    --bucket integration_tests_bucket \
    --precision s \
    --file /usr/src/performance_data.txt

if ! docker compose up api-test --exit-code-from api-test --no-log-prefix --quiet-pull; then
    die "tests failure"
fi

echo "$me: success 🎉"
teardown
