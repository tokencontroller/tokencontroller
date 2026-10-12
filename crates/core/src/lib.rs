//! The accounting core shared by the service and the `tc` command (ADR 0010).
//!
//! A pure library: no database, no network, no clock. Everything that has to
//! add up (money, splits, reconciliation, rules) lives here so the service and
//! the command line compute the same answer.

pub mod extension;
pub mod history;
pub mod money;
pub mod period;
pub mod status;
pub mod tag_map;
pub mod tags;
pub mod tokens;

pub use extension::{Extension, NoExtension};
pub use history::{History, Version};
pub use money::{Currency, ExchangeRate, Money, MoneyError};
pub use period::Period;
pub use status::{Arrival, CostSource, Status};
pub use tag_map::{CostCenter, CostKind, CostObject, TagMap};
pub use tags::{Filter, TagName, TagValue, Tags};
pub use tokens::{TokenCounts, TokenKind};
