//! Inbound-trigger subsystem.
//!
//! This module owns the path events travel when a plugin (or, later, a cloud
//! relay) reports an external event — for example a Discord `MESSAGE_CREATE`
//! or a Slack message — that should cause Orbit to run a workflow or wake an
//! agent.
//!
//! Flow:
//!   1. Plugin subprocess calls JSON-RPC `trigger.emit` on its per-plugin
//!      unix socket (see `plugins::core_api`).
//!   2. The socket handler hands the payload to [`Dispatcher::dispatch`].
//!   3. The dispatcher dedupes by `eventId` and fans out to matching
//!      workflows and matching per-agent `listen_bindings`.
//!
//! Workflow invocation is delegated to the orchestrator and agent invocation
//! to the agent runner (see `DispatchBindings`), keeping the dispatcher itself
//! as the fan-out + dedupe primitive.

pub mod bindings;
pub mod channel_session;
pub mod dispatcher;
pub mod reply_registry;
pub mod spawn;
pub mod subscriptions;
