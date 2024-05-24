#[cfg(test)]
use std::cell::RefCell;
use std::ops::Range;

use chrono::{DateTime, Days, NaiveDateTime, NaiveTime, TimeZone, Utc};

#[cfg(test)]
thread_local! {
    static OVERRIDE_NOW: RefCell<Option<DateTime<Utc>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn override_now(datetime: Option<DateTime<Utc>>) {
    OVERRIDE_NOW.set(datetime);
}

/// Wraps [`Utc::now`] in release builds.
///
/// In test builds, allows to use an overridden current date and time.
///
/// Will be part of chrono with https://github.com/chronotope/chrono/pull/1244.
pub(crate) fn utc_now() -> DateTime<Utc> {
    #[cfg(test)]
    if let Some(datetime) = OVERRIDE_NOW.take() {
        return datetime;
    }
    Utc::now()
}

/// Returns the date and time of current shift start and end, given a timezone
/// and a slice of shift start times.
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
pub(crate) fn find_shift_bounds<Tz>(
    timezone: &Tz,
    shift_start_times: &[NaiveTime],
) -> (DateTime<Tz>, DateTime<Tz>)
where
    Tz: TimeZone,
{
    let now = utc_now().with_timezone(timezone);
    let current_time = now.time();
    let found_start_index = shift_start_times
        .iter()
        .rposition(|&start_time| current_time >= start_time);
    let current_date = now.date_naive();
    let naive_shift_start = match found_start_index {
        Some(i) => current_date.and_time(shift_start_times[i]),
        None => {
            let previous_day = current_date - Days::new(1);
            previous_day.and_time(*shift_start_times.last().unwrap())
        }
    };
    let naive_shift_end = match found_start_index {
        Some(i) if i == shift_start_times.len() - 1 => {
            let next_day = current_date + Days::new(1);
            next_day.and_time(shift_start_times[0])
        }
        Some(i) => current_date.and_time(shift_start_times[i + 1]),
        None => current_date.and_time(shift_start_times[0]),
    };
    (
        naive_shift_start
            .and_local_timezone(timezone.clone())
            .unwrap(),
        naive_shift_end
            .and_local_timezone(timezone.clone())
            .unwrap(),
    )
}

/// Given a naive date and time envelope and a slice of naive time spans, returns
/// a vector of spans that fit entirely in the envelope.
pub(crate) fn apply_time_spans(
    envelope: Range<NaiveDateTime>,
    spans: &[(NaiveTime, NaiveTime)],
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut all_days_spans: Vec<(NaiveDateTime, NaiveDateTime)> = Vec::new();

    for (i, date) in envelope
        .start
        .date()
        .iter_days()
        .take_while(|date| date <= &envelope.end.date())
        .enumerate()
    {
        for (start, end) in spans {
            if start > end {
                if i == 0 {
                    all_days_spans.push((
                        date.pred_opt().unwrap().and_time(*start),
                        date.and_time(*end),
                    ));
                }
                all_days_spans.push((
                    date.and_time(*start),
                    date.succ_opt().unwrap().and_time(*end),
                ));
            } else {
                all_days_spans.push((date.and_time(*start), date.and_time(*end)));
            }
        }
    }
    all_days_spans.sort_by_key(|span| span.0);
    all_days_spans
        .into_iter()
        .filter_map(|(span_start, span_end)| {
            match (envelope.contains(&span_start), envelope.contains(&span_end)) {
                (true, true) if span_start < span_end => Some((span_start, span_end)),
                (true, false) if span_start < envelope.end => Some((span_start, envelope.end)),
                (false, true) if envelope.start < span_end => Some((envelope.start, span_end)),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    mod determine_shift_start {
        use chrono_tz::Etc::{GMTMinus2, GMTMinus4, GMTPlus1, GMTPlus2, GMTPlus3};

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
            override_now(Some("1984-12-09T01:15:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-08T09:00:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-09T09:00:00Z".parse().unwrap();
            let shifts = &[NaiveTime::from_hms_opt(11, 0, 0).unwrap()];
            let result = find_shift_bounds(&GMTMinus2, shifts);
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn one_shift_after_start() {
            override_now(Some("1984-12-09T13:15:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T13:00:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-10T13:00:00Z".parse().unwrap();
            let shifts = &[NaiveTime::from_hms_opt(11, 0, 0).unwrap()];
            let result = find_shift_bounds(&GMTPlus2, shifts);
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn on_first_shift_start() {
            override_now(Some("1984-12-09T01:15:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T01:15:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-09T09:30:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTMinus2, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn on_second_shift_start() {
            override_now(Some("1984-12-09T07:30:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T07:30:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-09T15:00:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTMinus4, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn on_third_shift_start() {
            override_now(Some("1984-12-09T20:00:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T20:00:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-10T04:15:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTPlus1, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn in_first_shift() {
            override_now(Some("1984-12-09T03:30:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T01:15:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-09T09:30:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTMinus2, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn in_second_shift() {
            override_now(Some("1984-12-09T14:30:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T14:30:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-09T22:00:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTPlus3, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn in_third_shift_before_midnight() {
            override_now(Some("1984-12-09T21:00:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T19:00:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-10T03:15:00Z".parse().unwrap();
            let result = find_shift_bounds(&Utc, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }

        #[test]
        fn in_third_shift_after_midnight() {
            override_now(Some("1984-12-10T03:00:00Z".parse().unwrap()));
            let expected_start: DateTime<Utc> = "1984-12-09T21:00:00Z".parse().unwrap();
            let expected_end: DateTime<Utc> = "1984-12-10T05:15:00Z".parse().unwrap();
            let result = find_shift_bounds(&GMTPlus2, &shift_times());
            assert_eq!(result.0, expected_start);
            assert_eq!(result.1, expected_end);
        }
    }

    mod apply_time_spans {
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
            let result = apply_time_spans(start..end, &excluded_spans());
            assert_eq!(result, &[]);
        }

        #[test]
        fn empty_spans_slice() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T05:00:00".parse().unwrap();
            let result = apply_time_spans(start..end, &[]);
            assert_eq!(result, &[]);
        }

        #[test]
        fn empty_span() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T12:00:00".parse().unwrap();
            let excluded = &[("08:00:00".parse().unwrap(), "08:00:00".parse().unwrap())];
            let result = apply_time_spans(start..end, excluded);
            assert_eq!(result, &[]);
        }

        #[test]
        fn no_span_applied() {
            let start = "1984-12-09T05:00:00".parse().unwrap();
            let end = "1984-12-09T12:00:00".parse().unwrap();
            let result = apply_time_spans(start..end, &excluded_spans());
            assert_eq!(result, &[]);
        }

        #[test]
        fn all_spans_applied_one_time() {
            let start = "1984-12-09T03:00:00".parse().unwrap();
            let end = "1984-12-10T02:00:00".parse().unwrap();
            let result = apply_time_spans(start..end, &excluded_spans());
            let expected = [
                (
                    "1984-12-09T04:00:00".parse().unwrap(),
                    "1984-12-09T05:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T12:00:00".parse().unwrap(),
                    "1984-12-09T12:20:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T19:00:00".parse().unwrap(),
                    "1984-12-09T20:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T23:00:00".parse().unwrap(),
                    "1984-12-10T01:00:00".parse().unwrap(),
                ),
            ];
            assert_eq!(result, expected);
        }

        #[test]
        fn all_spans_applied_three_time() {
            let start = "1984-12-09T03:00:00".parse().unwrap();
            let end = "1984-12-12T02:00:00".parse().unwrap();
            let result = apply_time_spans(start..end, &excluded_spans());
            let expected = [
                (
                    "1984-12-09T04:00:00".parse().unwrap(),
                    "1984-12-09T05:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T12:00:00".parse().unwrap(),
                    "1984-12-09T12:20:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T19:00:00".parse().unwrap(),
                    "1984-12-09T20:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T23:00:00".parse().unwrap(),
                    "1984-12-10T01:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-10T04:00:00".parse().unwrap(),
                    "1984-12-10T05:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-10T12:00:00".parse().unwrap(),
                    "1984-12-10T12:20:00".parse().unwrap(),
                ),
                (
                    "1984-12-10T19:00:00".parse().unwrap(),
                    "1984-12-10T20:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-10T23:00:00".parse().unwrap(),
                    "1984-12-11T01:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-11T04:00:00".parse().unwrap(),
                    "1984-12-11T05:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-11T12:00:00".parse().unwrap(),
                    "1984-12-11T12:20:00".parse().unwrap(),
                ),
                (
                    "1984-12-11T19:00:00".parse().unwrap(),
                    "1984-12-11T20:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-11T23:00:00".parse().unwrap(),
                    "1984-12-12T01:00:00".parse().unwrap(),
                ),
            ];
            assert_eq!(result, expected);
        }

        #[test]
        fn envelope_starts_in_span() {
            let start = "1984-12-09T04:40:00".parse().unwrap();
            let end = "1984-12-09T13:00:00".parse().unwrap();
            let result = apply_time_spans(start..end, &excluded_spans());
            let expected = [
                (
                    "1984-12-09T04:40:00".parse().unwrap(),
                    "1984-12-09T05:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T12:00:00".parse().unwrap(),
                    "1984-12-09T12:20:00".parse().unwrap(),
                ),
            ];
            assert_eq!(result, expected);
        }

        #[test]
        fn envelope_ends_in_span() {
            let start = "1984-12-09T18:00:00".parse().unwrap();
            let end = "1984-12-09T23:30:00".parse().unwrap();
            let result = apply_time_spans(start..end, &excluded_spans());
            let expected = [
                (
                    "1984-12-09T19:00:00".parse().unwrap(),
                    "1984-12-09T20:00:00".parse().unwrap(),
                ),
                (
                    "1984-12-09T23:00:00".parse().unwrap(),
                    "1984-12-09T23:30:00".parse().unwrap(),
                ),
            ];
            assert_eq!(result, expected);
        }
    }
}
