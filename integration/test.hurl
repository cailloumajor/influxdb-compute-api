GET {{host}}/timeline/id1

HTTP 200
Content-Type: application/msgpack
[Asserts]
bytes count == 535
bytes startsWith hex,dc;


GET {{host}}/performance/id2
Client-Timezone: Etc/GMT-2

HTTP 200
Content-Type: application/json
[Asserts]
jsonpath "$" > 48.7
jsonpath "$" < 48.8


GET {{host}}/shift-objective/id3
Client-Timezone: UTC

HTTP 200
Content-Type: application/json
[Asserts]
jsonpath "$" count == 6
jsonpath "$[0].t" > 0
jsonpath "$[0].v" == 0
jsonpath "$[1].t" > 0
jsonpath "$[1].v" == 756
jsonpath "$[2].t" > 0
jsonpath "$[2].v" == 756
jsonpath "$[3].t" > 0
jsonpath "$[3].v" == 1428
jsonpath "$[4].t" > 0
jsonpath "$[4].v" == 1428
jsonpath "$[5].t" > 0
jsonpath "$[5].v" == 2814


GET {{host}}/week-objective/id3
Client-Timezone: UTC

HTTP 200
Content-Type: application/json
[Asserts]
jsonpath "$" count == 18
jsonpath "$[0].t" > 0
jsonpath "$[0].v" == 0
jsonpath "$[-1:].v" nth 0 == 8442
