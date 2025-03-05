use std::iter;

use chrono::{Datelike, Days, NaiveDateTime, NaiveTime};
use chrono_tz::Tz;
use serde::Serialize;
use tokio::task::JoinHandle;
use tracing::{Instrument, error, info, info_span};

use crate::channel::{RoundtripSender, roundtrip_channel};
use crate::config_api::WeekStart;
use crate::time::{apply_time_spans, find_shift_bounds, utc_now};

pub(crate) struct ShiftObjectiveRequest {
    pub(crate) shift_start_times: Vec<NaiveTime>,
    pub(crate) pauses: Vec<(NaiveTime, NaiveTime)>,
    pub(crate) timezone: Tz,
    pub(crate) target_cycle_time: f32,
    pub(crate) target_efficiency: f32,
}

#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct ObjectivePoint {
    #[serde(rename = "t")]
    pub(crate) timestamp: i64,
    #[serde(rename = "v")]
    pub(crate) value: u16,
}

pub(crate) type ObjectiveData = Vec<ObjectivePoint>;

pub(crate) type ShiftObjectiveChannel = RoundtripSender<ShiftObjectiveRequest, ObjectiveData>;

pub(crate) struct WeekObjectiveRequest {
    pub(crate) shift_start_times: Vec<NaiveTime>,
    pub(crate) shift_engaged: Vec<bool>,
    pub(crate) pauses: Vec<(NaiveTime, NaiveTime)>,
    pub(crate) week_start: WeekStart,
    pub(crate) timezone: Tz,
    pub(crate) target_cycle_time: f32,
    pub(crate) target_efficiency: f32,
}

pub(crate) type WeekObjectiveChannel = RoundtripSender<WeekObjectiveRequest, ObjectiveData>;

struct NaivePoints {
    inner: Vec<(NaiveDateTime, u16)>,
    production_rate: f32,
}

impl NaivePoints {
    fn new(start: NaiveDateTime, cycle_time: f32, efficiency: f32) -> Self {
        let production_rate = 1.0 / cycle_time * efficiency;
        Self {
            inner: vec![(start, 0)],
            production_rate,
        }
    }

    fn push_shift(
        &mut self,
        shift_end: NaiveDateTime,
        engaged: bool,
        pauses: &[(NaiveTime, NaiveTime)],
    ) {
        let (mut last_datetime, mut quantity) = *self.inner.last().unwrap();
        let applied_pauses = engaged
            .then_some(apply_time_spans(last_datetime..shift_end, pauses))
            .unwrap_or_default();
        let interest_points = applied_pauses
            .into_iter()
            .flat_map(|(pause_start, pause_end)| [(pause_start, true), (pause_end, false)])
            .chain(iter::once((shift_end, engaged)));
        for (date_time, produced) in interest_points {
            let elapsed = (date_time - last_datetime).num_seconds() as f32;
            last_datetime = date_time;
            quantity += produced
                .then_some((elapsed * self.production_rate).floor() as u16)
                .unwrap_or_default();
            self.inner.push((date_time, quantity));
        }
    }

    fn into_objective_data(self, timezone: Tz) -> ObjectiveData {
        self.inner
            .into_iter()
            .map(|(date_time, value)| {
                let timestamp = date_time.and_local_timezone(timezone).unwrap().timestamp();
                ObjectivePoint { timestamp, value }
            })
            .collect()
    }
}

pub(crate) struct ProductionObjective;

impl ProductionObjective {
    pub(crate) fn handle_shift_objective(&self) -> (ShiftObjectiveChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel::<ShiftObjectiveRequest, ObjectiveData>(10);

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, _, reply_tx)) = rx.recv().await {
                    let shift_span =
                        find_shift_bounds(&request.timezone, &request.shift_start_times);
                    let shift_start = shift_span.0.naive_local();
                    let shift_end = shift_span.1.naive_local();
                    let mut naive_points = NaivePoints::new(
                        shift_start,
                        request.target_cycle_time,
                        request.target_efficiency,
                    );
                    naive_points.push_shift(shift_end, true, &request.pauses);
                    let objective_points = naive_points.into_objective_data(request.timezone);
                    if reply_tx.send(objective_points).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("shift_objective_handler")),
        );

        (tx, task)
    }

    pub(crate) fn handle_week_objective(&self) -> (WeekObjectiveChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel::<WeekObjectiveRequest, ObjectiveData>(10);

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, _, reply_tx)) = rx.recv().await {
                    let now_naive = utc_now().with_timezone(&request.timezone).date_naive();
                    let week_start_day = {
                        let now_weekday = now_naive.weekday().num_days_from_monday();
                        let week_start_weekday = request.week_start.day.num_days_from_monday();
                        let days_back = now_weekday - week_start_weekday;
                        now_naive - Days::new(days_back.into())
                    };
                    let mut shifts_iter = request
                        .shift_start_times
                        .iter()
                        .cycle()
                        .enumerate()
                        // The first member of the tuple item becomes the number of days to add.
                        .map(|(i, shift_start)| (i / request.shift_start_times.len(), shift_start))
                        .skip(request.week_start.shift_index)
                        .zip(iter::once(true).chain(request.shift_engaged))
                        .map(|((days_to_add, &shift_start_time), engaged)| {
                            let date_time = week_start_day
                                .checked_add_days(Days::new(days_to_add as u64))
                                .unwrap()
                                .and_time(shift_start_time);
                            (date_time, engaged)
                        });
                    let (first_datetime, _) = shifts_iter.next().unwrap();
                    let mut naive_points = NaivePoints::new(
                        first_datetime,
                        request.target_cycle_time,
                        request.target_efficiency,
                    );
                    for (shift_end, engaged) in shifts_iter {
                        naive_points.push_shift(shift_end, engaged, &request.pauses);
                    }
                    let objective_points = naive_points.into_objective_data(request.timezone);
                    if reply_tx.send(objective_points).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("week_objective_handler")),
        );

        (tx, task)
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::UTC;

    use crate::time::override_now;

    use super::*;

    fn start_times_fixture() -> Vec<NaiveTime> {
        vec![
            "05:30:00".parse().unwrap(),
            "13:30:00".parse().unwrap(),
            "21:30:00".parse().unwrap(),
        ]
    }

    fn pauses_fixture() -> Vec<(NaiveTime, NaiveTime)> {
        vec![
            ("08:00:00".parse().unwrap(), "08:20:00".parse().unwrap()),
            ("11:00:00".parse().unwrap(), "11:30:00".parse().unwrap()),
            ("16:00:00".parse().unwrap(), "16:20:00".parse().unwrap()),
            ("19:00:00".parse().unwrap(), "19:30:00".parse().unwrap()),
            ("00:00:00".parse().unwrap(), "00:20:00".parse().unwrap()),
            ("03:00:00".parse().unwrap(), "03:30:00".parse().unwrap()),
        ]
    }

    mod handle_shift_objective {
        use super::*;

        #[tokio::test]
        async fn now_in_first_shift() {
            override_now(Some("1984-12-09T07:00:00Z".parse().unwrap()));
            let request = ShiftObjectiveRequest {
                shift_start_times: start_times_fixture(),
                pauses: pauses_fixture(),
                timezone: UTC,
                target_cycle_time: 70.0,
                target_efficiency: 0.8,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_shift_objective();
            let points = channel.roundtrip(request).await.unwrap();
            assert_eq!(
                points,
                [
                    ObjectivePoint {
                        timestamp: 471418200,
                        value: 0,
                    },
                    ObjectivePoint {
                        timestamp: 471427200,
                        value: 102,
                    },
                    ObjectivePoint {
                        timestamp: 471428400,
                        value: 102,
                    },
                    ObjectivePoint {
                        timestamp: 471438000,
                        value: 211,
                    },
                    ObjectivePoint {
                        timestamp: 471439800,
                        value: 211,
                    },
                    ObjectivePoint {
                        timestamp: 471447000,
                        value: 293,
                    },
                ]
            );
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn no_pause() {
            override_now(Some("1984-12-09T13:29:59Z".parse().unwrap()));
            let request = ShiftObjectiveRequest {
                shift_start_times: start_times_fixture(),
                pauses: Vec::new(),
                timezone: UTC,
                target_cycle_time: 1.0,
                target_efficiency: 1.0,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_shift_objective();
            let points = channel.roundtrip(request).await.unwrap();
            assert_eq!(
                points,
                [
                    ObjectivePoint {
                        timestamp: 471418200,
                        value: 0,
                    },
                    ObjectivePoint {
                        timestamp: 471447000,
                        value: 28800,
                    },
                ]
            );
            assert!(!task.is_finished());
        }
    }

    mod handle_week_objective {
        use super::*;

        #[tokio::test]
        async fn first_engagement_configuration() {
            override_now(Some("2023-09-19T14:00:00Z".parse().unwrap()));
            let week_start = WeekStart {
                day: chrono::Weekday::Tue,
                shift_index: 1,
            };
            let request = WeekObjectiveRequest {
                shift_start_times: start_times_fixture(),
                shift_engaged: vec![true, false, true],
                pauses: pauses_fixture(),
                week_start,
                timezone: UTC,
                target_cycle_time: 60.0,
                target_efficiency: 1.0,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_week_objective();
            let points = channel.roundtrip(request).await.unwrap();
            assert_eq!(
                points,
                [
                    ObjectivePoint {
                        timestamp: 1695130200,
                        value: 0,
                    },
                    ObjectivePoint {
                        timestamp: 1695139200,
                        value: 150,
                    },
                    ObjectivePoint {
                        timestamp: 1695140400,
                        value: 150,
                    },
                    ObjectivePoint {
                        timestamp: 1695150000,
                        value: 310,
                    },
                    ObjectivePoint {
                        timestamp: 1695151800,
                        value: 310,
                    },
                    ObjectivePoint {
                        timestamp: 1695159000,
                        value: 430,
                    },
                    ObjectivePoint {
                        timestamp: 1695187800,
                        value: 430,
                    },
                    ObjectivePoint {
                        timestamp: 1695196800,
                        value: 580,
                    },
                    ObjectivePoint {
                        timestamp: 1695198000,
                        value: 580,
                    },
                    ObjectivePoint {
                        timestamp: 1695207600,
                        value: 740,
                    },
                    ObjectivePoint {
                        timestamp: 1695209400,
                        value: 740,
                    },
                    ObjectivePoint {
                        timestamp: 1695216600,
                        value: 860,
                    },
                ]
            );
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn second_engagement_configuration() {
            override_now(Some("2023-09-19T14:00:00Z".parse().unwrap()));
            let week_start = WeekStart {
                day: chrono::Weekday::Tue,
                shift_index: 1,
            };
            let request = WeekObjectiveRequest {
                shift_start_times: start_times_fixture(),
                shift_engaged: vec![false, true, false],
                pauses: pauses_fixture(),
                week_start,
                timezone: UTC,
                target_cycle_time: 60.0,
                target_efficiency: 1.0,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_week_objective();
            let points = channel.roundtrip(request).await.unwrap();
            assert_eq!(
                points,
                [
                    ObjectivePoint {
                        timestamp: 1695130200,
                        value: 0,
                    },
                    ObjectivePoint {
                        timestamp: 1695159000,
                        value: 0,
                    },
                    ObjectivePoint {
                        timestamp: 1695168000,
                        value: 150,
                    },
                    ObjectivePoint {
                        timestamp: 1695169200,
                        value: 150,
                    },
                    ObjectivePoint {
                        timestamp: 1695178800,
                        value: 310,
                    },
                    ObjectivePoint {
                        timestamp: 1695180600,
                        value: 310,
                    },
                    ObjectivePoint {
                        timestamp: 1695187800,
                        value: 430,
                    },
                    ObjectivePoint {
                        timestamp: 1695216600,
                        value: 430,
                    },
                ]
            );
            assert!(!task.is_finished());
        }
    }
}
