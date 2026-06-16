use std::collections::HashMap;

use anyhow::Context;
use embedded_perfmon_transport::{
    Event, EventKind, ExecutorEvent, ExecutorEventKind, GlobalEvent, TaskEvent, TaskEventKind,
};
use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Serialize;

pub mod perfetto;

pub fn deserialize_events(mut bytes: &mut [u8]) -> anyhow::Result<Vec<Event<'_>>> {
    let mut events = Vec::new();

    while !bytes.is_empty() {
        let bytes_len = bytes.len();
        let (event, rest) = match Event::deserialize(bytes).context("deserializing event") {
            Ok(v) => v,
            Err(e) => {
                if bytes_len < 128 {
                    // We're near the end. Probably just an early termination of the byte stream, so just ignore
                    return Ok(events);
                } else {
                    eprintln!(
                        "{} events deserialized before error: {events:?}",
                        events.len()
                    );
                    return Err(e);
                }
            }
        };
        bytes = rest;
        events.push(event);
    }

    Ok(events)
}

#[derive(Serialize, JsonSchema, Default)]
pub struct Capture {
    pub tickrate: u64,
    pub irq_states: IndexMap<u16, Vec<TimedValue<bool>>>,
    pub global_markers: Vec<TimedValue<String>>,
    pub global_spans: Vec<Span>,
    pub executor_states: IndexMap<u32, Vec<TimedValue<ExecutorState>>>,
    pub tasks: IndexMap<u32, Task>,
}

impl Capture {
    pub fn parse_events(events: &[Event<'_>]) -> Self {
        let mut capture = Self::default();

        let mut executor_map = HashMap::new();
        let mut inflight_global_spans = IndexMap::new();
        let mut inflight_task_spans = HashMap::new();

        for event in events {
            match &event.kind {
                EventKind::Global(global_event) => match global_event {
                    GlobalEvent::TickRate { rate } => capture.tickrate = *rate,
                    GlobalEvent::IrqStart { irq } => {
                        capture
                            .irq_states
                            .entry(*irq)
                            .or_default()
                            .push(TimedValue {
                                timestamp: event.timestamp,
                                state: true,
                            });
                    }
                    GlobalEvent::IrqEnd { irq } => {
                        capture
                            .irq_states
                            .entry(*irq)
                            .or_default()
                            .push(TimedValue {
                                timestamp: event.timestamp,
                                state: false,
                            });
                    }
                    GlobalEvent::Marker { name } => {
                        capture.global_markers.push(TimedValue {
                            timestamp: event.timestamp,
                            state: name.to_string(),
                        });
                    }
                    GlobalEvent::SpanStart { name, id } => {
                        inflight_global_spans.insert(
                            id,
                            Span {
                                name: name.to_string(),
                                start: event.timestamp,
                                end: 0,
                            },
                        );
                    }
                    GlobalEvent::SpanEnd { id } => {
                        if let Some(mut span) = inflight_global_spans.shift_remove(id) {
                            span.end = event.timestamp;
                            capture.global_spans.push(span);
                        }
                    }
                },
                EventKind::Executor(executor_event) => {
                    Self::handle_executor_event(
                        capture
                            .executor_states
                            .entry(executor_event.executor_id)
                            .or_default(),
                        executor_event,
                        event.timestamp,
                    );
                }
                EventKind::Task(task_event) => {
                    let executor_id =
                        if let TaskEventKind::TaskNew { executor_id } = task_event.kind {
                            Some(
                                &*executor_map
                                    .entry(task_event.task_id)
                                    .or_insert(executor_id),
                            )
                        } else {
                            executor_map.get(&task_event.task_id)
                        };

                    Self::handle_task_event(
                        executor_id.map(|executor_id| {
                            capture.executor_states.entry(*executor_id).or_default()
                        }),
                        capture.tasks.entry(task_event.task_id).or_default(),
                        task_event,
                        event.timestamp,
                        inflight_task_spans.entry(task_event.task_id).or_default(),
                    );
                }
            }
        }

        let last_event_time = events.last().map_or(0, |event| event.timestamp);

        // Finish all inflight spans and just act like they ended together with the last event
        for (_, mut global_span) in inflight_global_spans {
            global_span.end = last_event_time;
            capture.global_spans.push(global_span);
        }
        for (task_id, inflight_task_spans) in inflight_task_spans {
            if let Some(task) = capture.tasks.get_mut(&task_id) {
                for (_, mut task_span) in inflight_task_spans {
                    task_span.end = last_event_time;
                    task.spans.push(task_span);
                }
            }
        }

        capture
    }

    fn handle_executor_event(
        executor_trace: &mut Vec<TimedValue<ExecutorState>>,
        event: &ExecutorEvent,
        timestamp: u64,
    ) {
        if executor_trace.is_empty() {
            // Executor always starts idle
            executor_trace.push(TimedValue {
                timestamp,
                state: ExecutorState::Idle,
            });
        }

        match event.kind {
            ExecutorEventKind::ExecutorPollStart => {
                executor_trace.push(TimedValue {
                    timestamp,
                    state: ExecutorState::Scheduling,
                });
            }
            ExecutorEventKind::ExecutorIdle => {
                executor_trace.push(TimedValue {
                    timestamp,
                    state: ExecutorState::Idle,
                });
            }
        }
    }

    fn handle_task_event(
        mut executor_trace: Option<&mut Vec<TimedValue<ExecutorState>>>,
        task: &mut Task,
        event: &TaskEvent<'_>,
        timestamp: u64,
        inflight_spans: &mut IndexMap<u32, Span>,
    ) {
        let task_id = event.task_id;

        if let Some(executor_trace) = executor_trace.as_mut()
            && executor_trace.is_empty()
        {
            // Executor always starts idle
            executor_trace.push(TimedValue {
                timestamp,
                state: ExecutorState::Idle,
            });
        }

        match event.kind {
            TaskEventKind::TaskNew { executor_id: _ } => {
                task.states.push(TimedValue {
                    timestamp,
                    state: TaskState::Spawned,
                });
            }
            TaskEventKind::TaskEnd => {
                if task.states.is_empty() {
                    eprintln!("Detected TaskEnd for non-existing task: {task_id}");
                }

                if !matches!(
                    task.states.last(),
                    Some(TimedValue {
                        state: TaskState::Running,
                        ..
                    })
                ) {
                    eprintln!(
                        "Detected TaskEnd for task that's not in the running state: {task_id}"
                    );
                }

                task.states.push(TimedValue {
                    timestamp,
                    state: TaskState::End,
                });
            }
            TaskEventKind::TaskExecBegin => {
                if task.states.is_empty() {
                    eprintln!("Detected TaskExecBegin for non-existing task: {task_id}");
                }

                if !matches!(
                    task.states.last(),
                    Some(TimedValue {
                        state: TaskState::Waiting,
                        ..
                    })
                ) {
                    eprintln!(
                        "Detected TaskExecBegin for task that's not in the waiting state: {task_id}"
                    );
                }

                task.states.push(TimedValue {
                    timestamp,
                    state: TaskState::Running,
                });

                if let Some(executor_trace) = executor_trace.as_mut() {
                    executor_trace.push(TimedValue {
                        timestamp,
                        state: ExecutorState::Polling { task_id },
                    });
                }
            }
            TaskEventKind::TaskExecEnd => {
                if task.states.is_empty() {
                    eprintln!("Detected TaskExecEnd for non-existing task: {task_id}");
                }

                if !matches!(
                    task.states.last(),
                    Some(TimedValue {
                        state: TaskState::Running,
                        ..
                    })
                ) {
                    eprintln!(
                        "Detected TaskExecEnd for task that's not in the running state: {task_id}"
                    );
                }

                task.states.push(TimedValue {
                    timestamp,
                    state: TaskState::Idle,
                });

                if let Some(executor_trace) = executor_trace.as_mut() {
                    executor_trace.push(TimedValue {
                        timestamp,
                        state: ExecutorState::Scheduling,
                    });
                }
            }
            TaskEventKind::TaskReadyBegin => {
                if task.states.is_empty() {
                    eprintln!("Detected TaskReadyBegin for non-existing task: {task_id}");
                }

                match task.states.last() {
                    Some(TimedValue {
                        state: TaskState::Running,
                        ..
                    }) => {
                        task.states.push(TimedValue {
                            timestamp,
                            state: TaskState::RunningWaiting,
                        });
                    }
                    Some(TimedValue {
                        state: TaskState::Idle | TaskState::Spawned,
                        ..
                    }) => {
                        task.states.push(TimedValue {
                            timestamp,
                            state: TaskState::Waiting,
                        });
                    }
                    _ => {
                        eprintln!(
                            "Detected TaskReadyBegin for task that's not in the running, idle or spawned state: {task_id}"
                        );
                    }
                }
            }
            TaskEventKind::TaskNamed { name } => {
                task.names.push(TimedValue {
                    timestamp,
                    state: name.to_string(),
                });
            }
            TaskEventKind::Marker { name } => {
                task.markers.push(TimedValue {
                    timestamp,
                    state: name.to_string(),
                });
            }
            TaskEventKind::SpanStart { name, id } => {
                inflight_spans.insert(
                    id,
                    Span {
                        name: name.to_string(),
                        start: timestamp,
                        end: 0,
                    },
                );
            }
            TaskEventKind::SpanEnd { id } => {
                if let Some(mut span) = inflight_spans.shift_remove(&id) {
                    span.end = timestamp;
                    task.spans.push(span);
                }
            }
            TaskEventKind::PrioritySet { priority } => {
                task.priorities.push(TimedValue {
                    timestamp,
                    state: priority,
                });
            }
            TaskEventKind::DeadlineSet { deadline } => {
                task.deadlines.push(TimedValue {
                    timestamp,
                    state: deadline,
                });
            }
        }
    }
}

#[derive(Serialize, JsonSchema, Default)]
pub struct Task {
    pub names: Vec<TimedValue<String>>,
    pub markers: Vec<TimedValue<String>>,
    pub spans: Vec<Span>,
    pub states: Vec<TimedValue<TaskState>>,
    pub priorities: Vec<TimedValue<u8>>,
    pub deadlines: Vec<TimedValue<u64>>,
}

#[derive(Serialize, JsonSchema)]
pub enum TaskState {
    Spawned,
    Waiting,
    Running,
    /// Currently running, but also waiting to be polled again already
    RunningWaiting,
    Idle,
    End,
}

#[derive(Serialize, JsonSchema)]
pub enum ExecutorState {
    Idle,
    Scheduling,
    Polling { task_id: u32 },
}

#[derive(Serialize, JsonSchema)]
pub struct TimedValue<T> {
    pub timestamp: u64,
    pub state: T,
}

#[derive(Serialize, JsonSchema)]
pub struct Span {
    pub name: String,
    pub start: u64,
    pub end: u64,
}
