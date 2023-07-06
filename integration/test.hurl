GET http://influxdb-compute-api:8080/timeline/id1

HTTP 200
Content-Type: application/msgpack
[Asserts]
bytes count == 535
bytes startsWith hex,dc;
