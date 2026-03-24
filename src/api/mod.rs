use axum::Router;

use crate::state::SharedState;

mod auth;
mod config;
mod door;
mod media;
mod places;
pub mod sse;

pub fn router() -> Router<SharedState> {
    Router::new()
        .merge(auth::router())
        .merge(places::router())
        .merge(media::router())
        .merge(door::router())
        .merge(config::router())
        .merge(sse::router())
}
