//! Software development kit for Tower.md: baselines, tasks, runs, finish
//! evidence, proposals, cold start and verification, attached to the core
//! through `towermd_core::kit::Kit`.
pub mod cold_start;
mod cold_start_verification;
pub mod verifier;

pub mod baseline;
pub mod draft_store;
pub mod review;
pub mod task_model;

pub mod baseline_publication;
pub mod review_verification;
pub mod task_finish;
pub mod proposal_model;
pub mod notemd_summary;
pub(crate) mod task_alerts;

pub mod dev_state;
pub mod run_report;
pub mod run_record;
pub mod budget;
pub mod harness;
pub mod runner;
mod delegation_kit;
