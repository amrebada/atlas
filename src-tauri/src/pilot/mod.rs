//! Atlas Pilot — automated project-lifecycle orchestration.
//!
//! A "pilot" project is created by Atlas, planned via a gated planning
//! session (grill-me → PRD → epics), then implemented epic by epic by
//! wrapped, autonomous `claude` sessions. Atlas observes each session only
//! by tailing its JSONL transcript.
//!
//! Module layout:
//!   * `transcript` — read/interpret a session transcript (sentinels, todos).
//!   * `orchestrator` — the session state machine (spawn, auto-continue,
//!     pause, auto-advance, commit/push, crash-resume). Added next.
//!
//! On-disk pilot state lives in `storage::pilot_io`.

pub mod orchestrator;
pub mod transcript;

pub use orchestrator::{PilotManager, RunMode};
