use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};

use event::Attendance;
use libsql::Builder;
use sol_util::mainframe::{Event, EventJsonBody, Profile};
use toml::Table;

use std::{fs, sync::Arc};

mod database;
mod event;
mod roblox;
mod util;

#[derive(Clone)]
struct AppState {
    token: String,
    url: String,
}

async fn get_profile(
    State(state): State<AppState>,
    Path(user_id): Path<u64>,
) -> Json<Option<Profile>> {
    println!("Retrieving profile for {user_id}");
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let sol_rank_id = match roblox::get_rank_in_group(roblox::SOL_GROUP_ID, user_id).await {
        Ok(None) => {
            println!("Profile {user_id} retrieval failed, not in SOL");
            return Json(None);
        }
        Ok(Some((id, _))) => id,
        Err(e) => panic!("{}", e.to_string()),
    };

    let (profile, in_db) = database::get_profile(user_id, sol_rank_id, &conn).await;
    if in_db {
        println!("Retrieved {profile:?}");
        return Json(Some(profile));
    }

    println!("Profile {user_id} retrieval failed, no profile found");
    Json(None)
}

async fn get_attended(State(state): State<AppState>, Path(user_id): Path<u64>) -> Json<u64> {
    println!("Counting events attended for {user_id}");
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let count = database::get_attended(user_id, conn).await;
    println!("{user_id} has attended {count} events");
    Json(count)
}

async fn get_events_attended(
    State(state): State<AppState>,
    Path(user_id): Path<u64>,
) -> Json<Vec<u64>> {
    println!("Retrieving event ids for user {user_id}");
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let events = database::get_events_attended(user_id, conn).await;
    println!("{user_id} has attended {events:?}");
    Json(events)
}

async fn get_event_info_by_info(
    State(state): State<AppState>,
    Path(event_id): Path<i32>,
) -> Json<Option<Event>> {
    println!("Getting event {event_id}");
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let event = database::get_event(event_id, conn).await.unwrap_or(None);
    println!("Got event {event:?}");
    Json(event)
}

async fn put_event(State(state): State<AppState>, Json(body): Json<EventJsonBody>) -> StatusCode {
    println!(
        "Processing event hosted by {} at {}",
        body.host, body.location
    );
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let event = Event::from_json_body(body);

    let attendance_string = serde_json::to_string(&event.attendance).unwrap();
    conn.execute("INSERT INTO events (host, attendance, event_date, kind, location) VALUES (?1, ?2, ?3, ?4, ?5)", (
        event.host,
        attendance_string,
        event.event_date.to_rfc3339(),
        event.kind.as_str(),
        event.location.as_str(),
    )).await.unwrap();

    let conn_arc = Arc::new(conn);
    event.log_attendance(conn_arc).await;

    println!("Logged {event:?}");
    StatusCode::OK
}

// gets the hosted events from the specified userid
async fn get_hosted(State(state): State<AppState>, Path(host_id): Path<u64>) -> Json<Vec<Event>> {
    println!("Retrieving events hosted by {host_id}");
    let db = Builder::new_remote(state.url, state.token)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let mut rows = conn
        .query("SELECT * FROM events WHERE host = ?1", [host_id])
        .await
        .unwrap();

    let mut events = Vec::new();
    while let Ok(Some(r)) = rows.next().await {
        events.push(Event::from_row(&r))
    }

    println!("Successfully retrieved events for {host_id}");
    Json(events)
}

#[tokio::main]
async fn main() {
    let secrets = fs::read_to_string("Secrets.toml").expect("Secrets.toml does not exist");
    let secrets_table = secrets.parse::<Table>().unwrap();

    let db_token_string = secrets_table.get("DB_TOKEN").unwrap().to_string();
    let db_url_string = secrets_table.get("DB_URL").unwrap().to_string();

    let db_token = util::strip_token(db_token_string);
    let db_url = util::strip_token(db_url_string);

    let state = AppState {
        token: db_token,
        url: db_url,
    };

    let app = Router::new()
        .route("/profiles/:id", get(get_profile))
        .route("/events/:id", get(get_hosted))
        .route("/events", put(put_event))
        .route("/events/attended/:id", get(get_events_attended))
        .route("/events/num-attended/:id", get(get_attended))
        .route("/events/info/:id", get(get_event_info_by_info))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
