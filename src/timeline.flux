import "influxdata/influxdb/schema"

filterFields = (r) =>
  r._field == "averageCycleTime" or
  r._field == "campChange" or
  r._field == "cycle" or
  r._field == "cycleTimeOver"

stoppedTime = __targetcycletimeplaceholder__ * 1.05

colorFromStatuses = (r) =>
  ({r with color:
    if r.cycle then
      if r.cycleTimeOver then
        0
      else if float(v: r.averageCycleTime) / 10.0 < stoppedTime then
        1
      else
        2
    else if r.campChange then
      3
    else
      0,
  })

from(bucket: "__bucketplaceholder__")
  |> range(start: -12h)
  |> filter(fn: (r) => r._measurement == "__measurementplaceholder__")
  |> filter(fn: (r) => r.id == "__idplaceholder__")
  |> filter(fn: filterFields)
  |> schema.fieldsAsCols()
  |> map(fn: colorFromStatuses)
  |> keep(columns: ["_time", "color"])
  |> aggregateWindow(every: 1m, fn: last, column: "color")
