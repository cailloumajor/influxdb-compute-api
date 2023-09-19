use chrono::NaiveTime;
use chrono_tz::Tz;
use serde::Serialize;
use tokio::task::JoinHandle;
use tracing::{error, info, info_span, Instrument};

use crate::channel::{roundtrip_channel, RoundtripSender};
use crate::time::{apply_time_spans, find_shift_start};

pub(crate) struct ShiftObjectiveRequest {
    pub(crate) shift_start_times: Vec<NaiveTime>,
    pub(crate) pauses: Vec<(NaiveTime, NaiveTime)>,
    pub(crate) timezone: Tz,
    pub(crate) target_cycle_time: f32,
    pub(crate) target_efficiency: f32,
}

#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct ShiftObjectivePoint {
    #[serde(rename = "t")]
    pub(crate) timestamp: i64,
    #[serde(rename = "v")]
    pub(crate) value: u16,
}

pub(crate) type ShiftObjectiveChannel =
    RoundtripSender<ShiftObjectiveRequest, Vec<ShiftObjectivePoint>>;

pub(crate) struct ProductionObjective;

impl ProductionObjective {
    pub(crate) fn handle_shift_objective(&self) -> (ShiftObjectiveChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel::<ShiftObjectiveRequest, Vec<ShiftObjectivePoint>>(10);

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, reply_tx)) = rx.recv().await {
                    let ShiftObjectiveRequest {
                        shift_start_times,
                        pauses,
                        timezone,
                        target_cycle_time,
                        target_efficiency,
                    } = request;
                    let shift_span = find_shift_start(&timezone, &shift_start_times);
                    let shift_start = shift_span.0.naive_local();
                    let shift_end = shift_span.1.naive_local();
                    let applied_pauses = apply_time_spans(shift_start..shift_end, &pauses);
                    let mut time_points = vec![(shift_start, false)];
                    for (pause_start, pause_end) in applied_pauses {
                        time_points.push((pause_start, true));
                        time_points.push((pause_end, false));
                    }
                    if time_points.last().unwrap().0 < shift_end {
                        time_points.push((shift_end, true));
                    }
                    let production_rate = 1.0 / target_cycle_time * target_efficiency;
                    let mut objective_points: Vec<ShiftObjectivePoint> =
                        Vec::with_capacity(time_points.len());
                    for (date_time, producing) in time_points {
                        let timestamp = date_time.and_local_timezone(timezone).unwrap().timestamp();
                        let produced = if producing {
                            let elapsed_secs = objective_points
                                .last()
                                .map(|last_point| timestamp - last_point.timestamp)
                                .unwrap_or_default();
                            (elapsed_secs as f32 * production_rate).floor() as u16
                        } else {
                            0
                        };
                        let last_produced = objective_points
                            .last()
                            .map(|last_point| last_point.value)
                            .unwrap_or_default();
                        let value = last_produced + produced;
                        objective_points.push(ShiftObjectivePoint { timestamp, value });
                    }
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
}

#[cfg(test)]
mod tests {
    use chrono_tz::UTC;
    use tokio::sync::oneshot;

    use super::*;

    mod handle_shift_objective {
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
            ]
        }

        #[tokio::test]
        async fn now_in_first_shift() {
            override_now(Some("1984-12-09T07:00:00Z".parse().unwrap()));
            let (tx, rx) = oneshot::channel();
            let request = ShiftObjectiveRequest {
                shift_start_times: start_times_fixture(),
                pauses: pauses_fixture(),
                timezone: UTC,
                target_cycle_time: 70.0,
                target_efficiency: 0.8,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_shift_objective();
            channel.send(request, tx).await;
            let points = rx.await.unwrap();
            assert_eq!(
                points,
                [
                    ShiftObjectivePoint {
                        timestamp: 471418200,
                        value: 0,
                    },
                    ShiftObjectivePoint {
                        timestamp: 471427200,
                        value: 102,
                    },
                    ShiftObjectivePoint {
                        timestamp: 471428400,
                        value: 102,
                    },
                    ShiftObjectivePoint {
                        timestamp: 471438000,
                        value: 211,
                    },
                    ShiftObjectivePoint {
                        timestamp: 471439800,
                        value: 211,
                    },
                    ShiftObjectivePoint {
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
            let (tx, rx) = oneshot::channel();
            let request = ShiftObjectiveRequest {
                shift_start_times: start_times_fixture(),
                pauses: Vec::new(),
                timezone: UTC,
                target_cycle_time: 1.0,
                target_efficiency: 1.0,
            };
            let actor = ProductionObjective;
            let (channel, task) = actor.handle_shift_objective();
            channel.send(request, tx).await;
            let points = rx.await.unwrap();
            assert_eq!(
                points,
                [
                    ShiftObjectivePoint {
                        timestamp: 471418200,
                        value: 0,
                    },
                    ShiftObjectivePoint {
                        timestamp: 471447000,
                        value: 28800,
                    },
                ]
            );
            assert!(!task.is_finished());
        }
    }
}
