//! The shared contract data in `contract/`, compiled in. The TypeScript SDK
//! reads the same files from the npm package.

/// `contract/envelope.schema.json`.
pub const ENVELOPE_SCHEMA: &str = include_str!("../../../contract/envelope.schema.json");
/// `contract/error-codes.json`.
pub const ERROR_CODES: &str = include_str!("../../../contract/error-codes.json");
/// `contract/op-classes.json`.
pub const OP_CLASSES: &str = include_str!("../../../contract/op-classes.json");
/// `contract/agent-markers.json`.
pub const AGENT_MARKERS: &str = include_str!("../../../contract/agent-markers.json");
/// `contract/helper-argv.v0.8.1.json`.
pub const HELPER_ARGV_V0_8_1: &str = include_str!("../../../contract/helper-argv.v0.8.1.json");
