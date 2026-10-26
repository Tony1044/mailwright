//! Microsoft Graph for Mailwright.
//!
//! - [`auth`]: signing in to Microsoft Entra ID (Exchange Online) as a public client: the
//!   authorization code flow with PKCE through the system browser and a loopback redirect, or the
//!   device code flow; refreshing tokens.
//! - [`tokens`]: keeping the refresh token in the operating system's credential store.
//! - [`client`]: the Graph calls Mailwright makes (mail folders, messages with delta sync, sending,
//!   attachments, calendars and events), with paging, token refresh and throttling handled.
//! - [`types`]: the Graph JSON shapes.
//!
//! Nothing here panics on bad input: every network or JSON problem is a [`GraphError`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod auth;
pub mod client;
pub mod http;
pub mod config;
pub mod discovery;
pub mod error;
pub mod tokens;
pub mod types;

pub use client::GraphClient;
pub use config::AccountConfig;
pub use error::GraphError;
