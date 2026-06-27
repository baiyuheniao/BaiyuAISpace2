// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Shared domain types re-exported here so that lower-level modules (e.g. db.rs)
// can import from this neutral location instead of reaching up into commands/.
// The canonical definitions still live in the command modules; only the
// re-exports live here.
pub use crate::commands::llm::{ChatMessage, ChatSession};
pub use crate::commands::mcp::{MCPServer, MCPServerType};
pub use crate::commands::skills::Skill;
