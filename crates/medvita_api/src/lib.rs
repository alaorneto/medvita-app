//! Typed client of the Medvita server API (`/api/v1/`, Django REST Framework on the Skeleton core).
//!
//! This crate is the only door of the app to the API (AGENTS.md). It knows the server conventions
//! (error body, pagination, ETag/If-Match, Idempotency-Key, rate limits, maintenance) and renews the
//! access token once on `401 invalid_token`. It never logs bodies, query strings or tokens.

pub mod auth;
pub mod client;
pub mod endpoints;
pub mod error;
pub mod models;
pub mod pagination;
pub mod scope;

pub use auth::{AccessTokenProvider, AuthFailure, StaticTokenProvider};
pub use client::{ApiClient, ETag, IdempotencyKey, Versioned, WriteOptions};
pub use error::ApiError;
pub use models::{ActiveApp, Me, Organization, Profile, ServerInfo};
pub use pagination::{CursorPage, Page};
pub use scope::{AppScope, OrgScope};
