//! The API over HTTP, which is the only part of it that needs axum.
//!
//! Every handler builds a [`Request`], hands it to whatever [`router`] was
//! given to ask with, and turns the [`Reply`] into a status and a JSON body.
//! Nothing here touches a world: the asking crosses to the simulation task
//! and comes back on a oneshot, which is what keeps the API on the one thread
//! allowed to touch one.
//!
//! **A token on every request**, compared in constant time, and nothing else
//! — no second rate limit, no per-route permission. Whoever holds the token
//! is the operator.

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::oneshot;

use super::{Reply, Request};
use crate::net::{Action, Level};
use crate::sim::PlayerId;

/// Send a request to wherever it is answered, and get a receiver for the
/// reply. What [`crate::server::ws`] supplies, so this file knows nothing of
/// its channel.
pub type Ask = dyn Fn(Request) -> oneshot::Receiver<Reply> + Send + Sync;

#[derive(Clone)]
struct Api {
    ask: Arc<Ask>,
    token: Arc<str>,
}

/// Every route, under `/api`, behind the token.
pub fn router(
    token: String,
    ask: impl Fn(Request) -> oneshot::Receiver<Reply> + Send + Sync + 'static,
) -> Router {
    let api = Api { ask: Arc::new(ask), token: token.into() };
    Router::new()
        .route("/api/rooms", get(rooms))
        .route("/api/rooms/{room}", get(room))
        .route("/api/rooms/{room}/bots", get(bots).post(add_bot))
        .route("/api/rooms/{room}/bots/{seat}", delete(remove_bot))
        .route("/api/rooms/{room}/seats", post(sit))
        .route("/api/rooms/{room}/seats/{seat}", get(seat).delete(remove_bot))
        .route("/api/rooms/{room}/seats/{seat}/act", post(act))
        .route("/api/rooms/{room}/chunks/{row}/{col}", get(chunk))
        .route("/api/rooms/{room}/cells", get(cells))
        .route("/api/rooms/{room}/standings", get(standings))
        .layer(middleware::from_fn_with_state(api.clone(), bearer))
        .with_state(api)
}

/// `Authorization: Bearer TOKEN`, or 401 with a reason.
async fn bearer(
    State(api): State<Api>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let given = bearer_token(headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()));
    if given.is_some_and(|given| same(given.as_bytes(), api.token.as_bytes())) {
        return next.run(request).await;
    }
    refuse(StatusCode::UNAUTHORIZED, "a bearer token is needed; the server was started with one")
}

/// The token out of an `Authorization` header, whatever case the scheme is in.
///
/// **A scheme is case-insensitive** — RFC 7235 — and a client sending `bearer`
/// is sending this scheme, not a different one. The token itself is not: it is
/// compared byte for byte by [`same`].
fn bearer_token(header: Option<&str>) -> Option<&str> {
    let (scheme, token) = header?.split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then_some(token)
}

/// Equal, in time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() != b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn refuse(status: StatusCode, why: impl Into<String>) -> Response {
    (status, Json(json!({ "error": why.into() }))).into_response()
}

async fn answer(api: &Api, req: Request) -> Response {
    match (api.ask)(req).await {
        Ok(reply) => {
            let status =
                StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(reply.body)).into_response()
        }
        // The simulation task is gone, which is the server shutting down.
        Err(_) => refuse(StatusCode::SERVICE_UNAVAILABLE, "the simulation is not answering"),
    }
}

/// A seat off the path, or a 400 that says what was there instead.
fn seat_number(text: &str) -> Result<PlayerId, Response> {
    text.parse::<u8>()
        .map(PlayerId)
        .map_err(|_| refuse(StatusCode::BAD_REQUEST, format!("\"{text}\" is not a seat's number")))
}

fn number(what: &str, text: &str) -> Result<i32, Response> {
    text.parse::<i32>()
        .map_err(|_| refuse(StatusCode::BAD_REQUEST, format!("\"{text}\" is not a {what}")))
}

/// A body that did not parse is a 400 in the same shape as every other
/// refusal, rather than axum's plain text.
fn body<T>(parsed: Result<Json<T>, JsonRejection>) -> Result<T, Response> {
    parsed.map(|Json(t)| t).map_err(|e| refuse(StatusCode::BAD_REQUEST, e.body_text()))
}

#[derive(Deserialize)]
struct AddBotBody {
    name: Option<String>,
    level: Option<Level>,
    team: Option<u8>,
}

#[derive(Deserialize)]
struct SitBody {
    name: String,
    team: Option<u8>,
}

#[derive(Deserialize)]
struct ActBody {
    action: Action,
}

#[derive(Deserialize)]
struct Window {
    r0: i32,
    c0: i32,
    r1: i32,
    c1: i32,
}

async fn rooms(State(api): State<Api>) -> Response {
    answer(&api, Request::Rooms).await
}

async fn room(State(api): State<Api>, Path(room): Path<String>) -> Response {
    answer(&api, Request::Room { room }).await
}

async fn bots(State(api): State<Api>, Path(room): Path<String>) -> Response {
    answer(&api, Request::Bots { room }).await
}

async fn add_bot(
    State(api): State<Api>,
    Path(room): Path<String>,
    parsed: Result<Json<AddBotBody>, JsonRejection>,
) -> Response {
    let AddBotBody { name, level, team } = match body(parsed) {
        Ok(b) => b,
        Err(why) => return why,
    };
    answer(&api, Request::AddBot { room, name, level, team: team.map(PlayerId) }).await
}

async fn remove_bot(
    State(api): State<Api>,
    Path((room, seat)): Path<(String, String)>,
) -> Response {
    match seat_number(&seat) {
        Ok(seat) => answer(&api, Request::RemoveBot { room, seat }).await,
        Err(why) => why,
    }
}

async fn sit(
    State(api): State<Api>,
    Path(room): Path<String>,
    parsed: Result<Json<SitBody>, JsonRejection>,
) -> Response {
    let SitBody { name, team } = match body(parsed) {
        Ok(b) => b,
        Err(why) => return why,
    };
    answer(&api, Request::Sit { room, name, team: team.map(PlayerId) }).await
}

async fn seat(State(api): State<Api>, Path((room, seat)): Path<(String, String)>) -> Response {
    match seat_number(&seat) {
        Ok(seat) => answer(&api, Request::Seat { room, seat }).await,
        Err(why) => why,
    }
}

async fn act(
    State(api): State<Api>,
    Path((room, seat)): Path<(String, String)>,
    parsed: Result<Json<ActBody>, JsonRejection>,
) -> Response {
    let seat = match seat_number(&seat) {
        Ok(seat) => seat,
        Err(why) => return why,
    };
    let ActBody { action } = match body(parsed) {
        Ok(b) => b,
        Err(why) => return why,
    };
    answer(&api, Request::Act { room, seat, action }).await
}

async fn chunk(
    State(api): State<Api>,
    Path((room, row, col)): Path<(String, String, String)>,
) -> Response {
    let (row, col) = match (number("row", &row), number("column", &col)) {
        (Ok(row), Ok(col)) => (row, col),
        (Err(why), _) | (_, Err(why)) => return why,
    };
    answer(&api, Request::Chunk { room, row, col }).await
}

async fn cells(
    State(api): State<Api>,
    Path(room): Path<String>,
    window: Result<Query<Window>, QueryRejection>,
) -> Response {
    let Query(Window { r0, c0, r1, c1 }) = match window {
        Ok(w) => w,
        Err(e) => return refuse(StatusCode::BAD_REQUEST, e.body_text()),
    };
    answer(&api, Request::Cells { room, r0, c0, r1, c1 }).await
}

async fn standings(State(api): State<Api>, Path(room): Path<String>) -> Response {
    answer(&api, Request::Standings { room }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scheme is the only part that is case-insensitive, and it has to be:
    /// a client sending `bearer` was refused with "a bearer token is needed",
    /// which reads as the token being wrong rather than its spelling.
    #[test]
    fn the_bearer_scheme_is_read_in_any_case() {
        assert_eq!(bearer_token(Some("Bearer sesame")), Some("sesame"));
        assert_eq!(bearer_token(Some("bearer sesame")), Some("sesame"));
        assert_eq!(bearer_token(Some("BEARER sesame")), Some("sesame"));
        assert_eq!(bearer_token(Some("Basic sesame")), None, "another scheme is another scheme");
        assert_eq!(bearer_token(Some("Bearer")), None, "a scheme with no token is no token");
        assert_eq!(bearer_token(None), None);
        assert_eq!(bearer_token(Some("Bearer SESAME")), Some("SESAME"), "the token is not folded");
    }
}
