#[path = "support/memory_harness.rs"]
mod memory_harness;

#[path = "memory/backend_compatibility.rs"]
mod backend_compatibility;
#[path = "memory/backend_parity.rs"]
mod backend_parity;
#[path = "memory/capability_contract.rs"]
mod capability_contract;
#[path = "memory/comparison.rs"]
mod comparison;
// TODO(v2): rewrite for v2 API (references agent::loop_::IntegrationTurnParams, providers::Provider)
// #[path = "memory/consolidation_orchestrator.rs"]
// mod consolidation_orchestrator;
#[path = "memory/delete_contract.rs"]
mod delete_contract;
#[path = "memory/governance.rs"]
mod governance;
#[path = "memory/governance_delete.rs"]
mod governance_delete;
#[path = "memory/governance_export.rs"]
mod governance_export;
#[path = "memory/inference_events.rs"]
mod inference_events;
#[path = "memory/layer_schema.rs"]
mod layer_schema;
#[path = "memory/markdown_tagged.rs"]
mod markdown_tagged;
#[path = "memory/provenance_validation.rs"]
mod provenance_validation;
// TODO(v2): rewrite for v2 API (references agent::loop_::build_context_for_integration)
// #[path = "memory/revocation_gate.rs"]
// mod revocation_gate;
#[path = "memory/sqlite_contract.rs"]
mod sqlite_contract;
#[path = "memory/sqlite_persistence.rs"]
mod sqlite_persistence;
#[path = "memory/sqlite_schema.rs"]
mod sqlite_schema;
#[path = "memory/sqlite_scoring.rs"]
mod sqlite_scoring;
#[path = "memory/sqlite_search.rs"]
mod sqlite_search;
// TODO(v2): rewrite for v2 API (references agent::loop_::, providers::Provider)
// #[path = "memory/tenant_recall.rs"]
// mod tenant_recall;
#[path = "memory/throughput.rs"]
mod throughput;
#[path = "memory/tool_contract.rs"]
mod tool_contract;
