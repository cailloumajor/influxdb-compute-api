GET {{host}}/timeline/id1

HTTP 200
Content-Type: application/msgpack
[Asserts]
bytes count == 535
bytes startsWith hex,dc;


GET {{host}}/performance/id2
Client-Time: 2023-08-21T12:59:59+02:00

HTTP 200
Content-Type: application/json
[Asserts]
jsonpath "$" > 48.5
jsonpath "$" < 48.6
