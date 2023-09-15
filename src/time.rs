use std::iter;
use std::ops::Range;

use chrono::{DateTime, Days, Duration, FixedOffset, NaiveDateTime, NaiveTime};

/// Determines the shift start of given timestamp (`current` argument),
/// given a slice of shift start times.
///
/// # Panics
///
/// This function will panic if:
///
/// * shift start times slice is empty;
/// * a shift start time can be inconsistent for `and_local_timezone` method on
///   [`NaiveDateTime`][chrono::naive::datetime::NaiveDateTime].
///
/// # Important
///
/// This function assumes that shift start times:
///
/// * are in chronological order;
/// * covers the entire day.
pub(crate) fn determine_shift_start(
    current: DateTime<FixedOffset>,
    shift_start_times: &[NaiveTime],
) -> DateTime<FixedOffset> {
    let current_time = current.time();
    let found_start_time = shift_start_times
        .iter()
        .rev()
        .find(|start_time| current_time >= **start_time);
    let current_date = current.date_naive();
    let naive_shift_start = match found_start_time {
        Some(time) => current_date.and_time(*time),
        None => {
            let previous_day = current_date - Days::new(1);
            previous_day.and_time(*shift_start_times.last().unwrap())
        }
    };
    naive_shift_start
        .and_local_timezone(current.timezone())
        .unwrap()
}

/// Calculates the total duration excluded from a given time envelope and
/// time spans to exclude.
pub(crate) fn excluded_duration(
    envelope: Range<NaiveDateTime>,
    excluded_spans: &[(NaiveTime, NaiveTime)],
) -> Duration {
    let mut spans_to_exclude: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();

    for (date, first) in envelope
        .start
        .date()
        .iter_days()
        .take_while(|date| date <= &envelope.end.date())
        .zip(iter::once(true).chain(iter::repeat(false)))
    {
        for (start, end) in excluded_spans {
            if start > end {
                if first {
                    spans_to_exclude.push((
                        date.pred_opt().unwrap().and_time(*start),
                        date.and_time(*end),
                    ));
                }
                spans_to_exclude.push((
                    date.and_time(*start),
                    date.succ_opt().unwrap().and_time(*end),
                ));
            } else {
                spans_to_exclude.push((date.and_time(*start), date.and_time(*end)));
            }
        }
    }

    spans_to_exclude
        .iter()
        .fold(Duration::zero(), |acc, (span_start, span_end)| {
            let duration_add = match (envelope.contains(span_start), envelope.contains(span_end)) {
                (true, true) => *span_end - *span_start,
                (true, false) => envelope.end - *span_start,
                (false, true) => *span_end - envelope.start,
                (false, false) => Duration::zero(),
            };
            acc + duration_add
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod determine_shift_start {
        use super::*;

        fn shift_times() -> Vec<NaiveTime> {
            vec![
                NaiveTime::from_hms_opt(3, 15, 0).unwrap(),
                NaiveTime::from_hms_opt(11, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(19, 0, 0).unwrap(),
            ]
        }

        #[test]
        fn one_shift_before_start() {
            let current: DateTime<FixedOffset> = "1984-12-09T03:15:00+02:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-08T09:00:00Z".parse().unwrap();
            let shifts = &[NaiveTime::from_hms_opt(11, 0, 0).unwrap()];
            let result = determine_shift_start(current, shifts);
            assert_eq!(result, expected);
        }

        #[test]
        fn one_shift_after_start() {
            let current: DateTime<FixedOffset> = "1984-12-09T11:15:00-02:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T13:00:00Z".parse().unwrap();
            let shifts = &[NaiveTime::from_hms_opt(11, 0, 0).unwrap()];
            let result = determine_shift_start(current, shifts);
            assert_eq!(result, expected);
        }

        #[test]
        fn on_first_shift_start() {
            let current: DateTime<FixedOffset> = "1984-12-09T03:15:00+02:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T01:15:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn on_second_shift_start() {
            let current: DateTime<FixedOffset> = "1984-12-09T11:30:00+04:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T07:30:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn on_third_shift_start() {
            let current: DateTime<FixedOffset> = "1984-12-09T19:00:00-01:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T20:00:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn in_first_shift() {
            let current: DateTime<FixedOffset> = "1984-12-09T05:30:00+02:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T01:15:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn in_second_shift() {
            let current: DateTime<FixedOffset> = "1984-12-09T11:30:00-03:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T14:30:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn in_third_shift_before_midnight() {
            let current: DateTime<FixedOffset> = "1984-12-09T21:00:00+00:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T19:00:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }

        #[test]
        fn in_third_shift_after_midnight() {
            let current: DateTime<FixedOffset> = "1984-12-10T01:00:00-02:00".parse().unwrap();
            let expected: DateTime<FixedOffset> = "1984-12-09T21:00:00Z".parse().unwrap();
            let result = determine_shift_start(current, &shift_times());
            assert_eq!(result, expected);
        }
    }

    mod effective_duration {
        use super::*;

        fn excluded_spans() -> Vec<(NaiveTime, NaiveTime)> {
            vec![
                ("23:00:00".parse().unwrap(), "01:00:00".parse().unwrap()),
                ("04:00:00".parse().unwrap(), "05:00:00".parse().unwrap()),
                ("12:00:00".parse().unwrap(), "12:20:00".parse().unwrap()),
                ("19:00:00".parse().unwrap(), "20:00:00".parse().unwrap()),
            ]
        }

        #[test]
        fn invalid_envelope() {
            let start = "1984-12-09T03:00:00".parse().unwrap();
            let end = "1984-12-09T02:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::minutes(0));
        }

        #[test]
        fn empty_excluded() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T05:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &[]);
            assert_eq!(result, Duration::zero());
        }

        #[test]
        fn zero_duration_excluded() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T12:00:00".parse().unwrap();
            let excluded = &[("08:00:00".parse().unwrap(), "08:00:00".parse().unwrap())];
            let result = excluded_duration(start..end, excluded);
            assert_eq!(result, Duration::zero());
        }

        #[test]
        fn no_excluded_in_envelope() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T12:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::zero());
        }

        #[test]
        fn all_excluded_one_time() {
            let start = "1984-12-09T03:00:00".parse().unwrap();
            let end = "1984-12-10T02:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::minutes(260));
        }

        #[test]
        fn all_excluded_three_time() {
            let start = "1984-12-09T03:00:00".parse().unwrap();
            let end = "1984-12-12T02:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::minutes(780));
        }

        #[test]
        fn envelope_starts_in_excluded() {
            let start = "1984-12-09T04:40:00".parse().unwrap();
            let end = "1984-12-09T13:00:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::minutes(40));
        }

        #[test]
        fn envelope_ends_in_excluded() {
            let start = "1984-12-09T18:00:00".parse().unwrap();
            let end = "1984-12-09T23:30:00".parse().unwrap();
            let result = excluded_duration(start..end, &excluded_spans());
            assert_eq!(result, Duration::minutes(90));
        }
    }
}
