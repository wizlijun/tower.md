//! Software development kit for Tower.md: baselines, tasks, runs, finish
//! evidence, proposals, cold start and verification, attached to the core
//! through `towermd_core::kit::Kit`.
pub mod cold_start;
pub mod imports;
mod louvain;
mod cold_start_verification;
pub mod verifier;

pub mod baseline;
pub mod baseline_origin;
pub mod draft_store;
pub mod review;
pub mod task_model;

pub mod baseline_publication;
pub mod review_verification;
pub mod task_finish;
pub mod proposal_model;
pub mod notemd_summary;
pub(crate) mod task_alerts;
pub mod task_interrupts;
pub mod task_reviews;
pub mod task_card;

pub mod dev_state;
pub mod run_report;
pub mod run_record;
pub mod budget;
pub mod growth;
pub mod harness;
pub mod runner;
mod delegation_kit;

/// FR-189: the software development starter kit, provided by this kit.
pub const STARTER: &str = include_str!("../starters/software.yaml");
