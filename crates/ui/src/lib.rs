//! Mailwright's interface: a classic desktop mail and calendar client, in egui.
//!
//! [`MailApp`] holds the state; [`MailApp::logic`] handles sync updates, helper threads and
//! shortcuts; [`MailApp::ui`] draws the window: title row, ribbon, folder pane, message list,
//! reading pane, calendar, status bar, and the compose and appointment windows.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(clippy::collapsible_if)]

pub mod alert;
pub mod app;
pub mod app_icon;
mod backstage;
pub mod calendar;
mod chrome;
pub mod compose;
mod dialogs;
pub mod event;
mod folders;
pub mod icons;
mod keys;
pub mod list;
pub mod notify;
mod reading;
pub mod render;
mod ribbon;
pub mod settings;
mod signin;
pub mod theme;
pub mod widgets;

pub use app::{MailApp, View};
pub use settings::Settings;
