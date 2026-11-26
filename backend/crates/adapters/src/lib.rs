//! Outbound adapters implementing application-owned ports.
//!
//! Technology-specific persistence and AWS code belongs here, not in domain
//! or application crates. Concrete production adapters will be added with their contracts.

pub mod in_memory;
pub mod repository_contracts;
