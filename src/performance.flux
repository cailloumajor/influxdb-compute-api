import "influxdata/influxdb/schema"

filterFields = (r) => r._field == "goodParts" or r._field == "partRef"

from(bucket: "__bucketplaceholder__")
  |> range(start: __startplaceholder__)
  |> filter(fn: (r) => r["_measurement"] == "__measurementplaceholder__")
  |> filter(fn: (r) => r.id == "__idplaceholder__")
  |> filter(fn: filterFields)
  |> aggregateWindow(every: 1m, fn: last)
  |> schema.fieldsAsCols()
  |> group(columns: ["partRef"])
  |> elapsed(unit: 1m)
  |> map(fn: (r) => ({r with elapsed: if not exists r.partRef then -1 else r.elapsed}))
  |> cumulativeSum(columns: ["elapsed"])
  |> increase(columns: ["goodParts"])
  |> last(column: "elapsed")
  |> rename(columns: {_time: "end"})
  |> keep(columns: ["elapsed", "end", "goodParts", "partRef"])
