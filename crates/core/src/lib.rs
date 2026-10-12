//! The accounting core shared by the service and the `tc` command (ADR 0010).
//!
//! A pure library: no database, no network, no clock. Everything that has to
//! add up (money, splits, reconciliation, rules) lives here so the service and
//! the command line compute the same answer.

pub mod directory;
pub mod entry;
pub mod extension;
pub mod history;
pub mod message;
pub mod money;
pub mod organization;
pub mod pattern;
pub mod period;
pub mod placement;
pub mod price;
pub mod rules;
pub mod status;
pub mod tag_map;
pub mod tags;
pub mod tokens;

pub use directory::{Directory, Person, Project, Ticket};
pub use entry::{HandShare, Segment, Split, Splits, TokenEntry};
pub use extension::{Extension, NoExtension};
pub use history::{History, Version};
pub use message::{Message, MessageId, ModelRef, SessionId, Signals, Usage};
pub use money::{Currency, ExchangeRate, Money, MoneyError};
pub use organization::Organization;
pub use period::Period;
pub use placement::{PlacedBy, Placement, PlacementRequest, SummaryMatcher};
pub use price::{Figure, ModelPrice, PriceTable, Valuation};
pub use rules::{Rule, SeveralKeys};
pub use status::{Arrival, CostSource, Status};
pub use tag_map::{CostCenter, CostKind, CostObject, TagMap};
pub use tags::{Filter, TagName, TagValue, Tags};
pub use tokens::{TokenCounts, TokenKind};
