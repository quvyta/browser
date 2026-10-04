//! The engine against a real headless Chromium and a local web server. No test reaches a real
//! site or the desktop.

mod browsing;
pub(crate) mod fixture;
mod helpers;
mod lifecycle;
mod selection;
