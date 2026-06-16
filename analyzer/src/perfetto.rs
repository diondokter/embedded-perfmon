use std::collections::HashSet;

use embedded_perfmon_transport::{Event, EventKind, GlobalEvent, TaskEventKind};
use perfetto_protos::idl::{
    TracePacket, TrackDescriptor, TrackEvent,
    trace_packet::{Data, OptionalTrustedPacketSequenceId},
    track_descriptor::StaticOrDynamicName,
    track_event::{NameField, Type},
};

pub fn to_perfetto_trace(events: &[Event]) -> perfetto_protos::idl::Trace {
    let mut trace = perfetto_protos::idl::Trace::default();

    let tickrate = events
        .iter()
        .find_map(|event| {
            if let EventKind::Global(GlobalEvent::TickRate { rate }) = event.kind {
                Some(rate)
            } else {
                None
            }
        })
        .unwrap_or(1_000_000);
    let nanos_per_tick = 1_000_000_000 / tickrate;

    for event in events {
        match &event.kind {
            EventKind::Global(_global_event) => {}
            EventKind::Executor(_executor_event) => {}
            EventKind::Task(task_event) => match task_event.kind {
                TaskEventKind::TaskNew { .. } => {
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceBegin.into()),
                            name_field: Some(NameField::Name("spawned".into())),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::TaskEnd => {
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceEnd.into()),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::TaskExecBegin => {
                    // Slice end of pending
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceEnd.into()),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });

                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceBegin.into()),
                            name_field: Some(NameField::Name("exec".into())),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::TaskExecEnd => {
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceEnd.into()),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::TaskReadyBegin => {
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackEvent(TrackEvent {
                            r#type: Some(Type::SliceBegin.into()),
                            name_field: Some(NameField::Name("pending".into())),
                            track_uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::TaskNamed { name } => {
                    trace.packet.push(TracePacket {
                        timestamp: Some(event.timestamp * nanos_per_tick),
                        optional_trusted_packet_sequence_id: Some(
                            OptionalTrustedPacketSequenceId::TrustedPacketSequenceId(
                                task_event.task_id,
                            ),
                        ),
                        data: Some(Data::TrackDescriptor(TrackDescriptor {
                            static_or_dynamic_name: Some(StaticOrDynamicName::Name(name.into())),
                            uuid: Some(task_event.task_id as u64),
                            ..Default::default()
                        })),
                        ..Default::default()
                    });
                }
                TaskEventKind::PrioritySet { .. } => {}
                TaskEventKind::DeadlineSet { .. } => {}
                TaskEventKind::Marker { .. } => {}
                TaskEventKind::SpanStart { .. } => {}
                TaskEventKind::SpanEnd { .. } => {}
            },
        }
    }

    trace
}
