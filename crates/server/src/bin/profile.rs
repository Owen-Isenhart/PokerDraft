//! Offline fixtures only; no credentials or state from a live installation.
#![allow(dead_code)]
#[path = "../security.rs"]
mod security;
#[path = "../service.rs"]
mod service;
#[path = "../store.rs"]
mod store;
fn main() {
    service::profile::run();
}
