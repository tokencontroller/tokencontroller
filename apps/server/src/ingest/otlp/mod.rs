//! The OpenTelemetry receiver's decoding: OTLP over HTTP, as protobuf or
//! JSON, turned into one [`TelemetryMessage`](super::model::TelemetryMessage)
//! per model call, per agent harness as the Collect pages describe. Events
//! that carry prompt text are dropped here, before anything is stored.
