use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Datelike, Utc, Weekday};
use libsql::Row;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::rank::{self, Rank};

const MAINFRAME_URL: &str = "http://localhost:3000";

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Event {
    pub host: u64,
    pub attendance: Vec<u64>, // List of userids that attended the event, including the host
    pub event_date: DateTime<Utc>,
    pub location: String,
    pub kind: String,
    pub metadata: Option<HashMap<String, HashMap<String, String>>>,
}

impl Event {
    pub fn new(host: u64, attendance: Vec<u64>, location: String, kind: String) -> Self {
        let event_date: DateTime<Utc> = Utc::now();
        Self {
            host,
            attendance,
            event_date,
            location,
            kind,
            metadata: None,
        }
    }

    pub fn from_row(row: &Row) -> Self {
        let _event_id = row.get::<u64>(0).unwrap();
        let host = row.get::<u64>(1).unwrap();
        let attendance = serde_json::from_str::<Vec<u64>>(row.get_str(2).unwrap()).unwrap();
        let event_date = chrono::DateTime::parse_from_rfc3339(row.get_str(3).unwrap()).unwrap();
        let location = row.get_str(4).unwrap();
        let kind = row.get_str(5).unwrap();
        let metadata_str = row.get::<Option<String>>(6).unwrap();
        let metadata: Option<HashMap<String, HashMap<String, String>>> =
            metadata_str.map(|s| serde_json::from_str(&s).unwrap());

        Self {
            host,
            attendance,
            event_date: event_date.into(),
            location: location.to_string(),
            kind: kind.to_string(),
            metadata,
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct EventJsonBody {
    pub host: u64,
    // pub attendance: Vec<u64>, // List of userids that attended the event, including the host
    pub names: Vec<String>,
    pub location: String,
    pub kind: String,
    pub metadata: Option<HashMap<String, HashMap<String, String>>>,
}

fn has_date_rolled_over(previous_date: DateTime<Utc>) -> bool {
    let current_date = Utc::now();

    let current_week = current_date.iso_week().week();
    let prev_week = previous_date.iso_week().week();
    if current_week != prev_week {
        return true;
    }

    if Weekday::Sun == previous_date.weekday() && Weekday::Sun != current_date.weekday() {
        return true;
    }

    false
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Profile {
    pub user_id: u64,
    pub username: Option<String>, // because this is a field added later, needs to be optional
    pub rank_id: u64,
    pub last_event_attended_date: Option<DateTime<Utc>>,
    pub total_marks: i32,
    pub marks_at_current_rank: i32,
    pub events_attended_this_week: i32,
}

impl Profile {
    pub fn new(user_id: u64, username: Option<String>, rank_id: u64) -> Self {
        Self {
            user_id,
            username,
            rank_id,
            last_event_attended_date: None,
            total_marks: 0,
            marks_at_current_rank: 0,
            events_attended_this_week: 0,
        }
    }
    pub fn from_row(row: &Row) -> Self {
        let user_id = row.get::<u64>(0).unwrap();
        let username = row.get::<Option<String>>(1).unwrap();
        let rank_id = row.get::<u64>(2).unwrap();
        let event_date_string = row.get::<String>(3).unwrap();
        let event_date_opt_string = if event_date_string == "null" {
            None
        } else {
            Some(event_date_string)
        };
        let last_event_attended_date = event_date_opt_string
            .map(|s| chrono::DateTime::parse_from_rfc3339(&s).unwrap().to_utc());
        let total_marks = row.get::<i32>(4).unwrap();
        let marks_at_current_rank = row.get::<i32>(5).unwrap();
        let events_attended_this_week = row.get::<i32>(6).unwrap();

        Self {
            user_id,
            username,
            rank_id,
            last_event_attended_date,
            total_marks,
            marks_at_current_rank,
            events_attended_this_week,
        }
    }

    pub fn should_promote(&self) -> bool {
        let rank = Rank::from_rank_id(self.rank_id).unwrap();
        if let Some(marks) = rank.required_marks() {
            return self.marks_at_current_rank == marks;
        }

        false
    }

    pub fn try_award_mark(&mut self) -> bool {
        if self.events_attended_this_week == rank::EVENT_PER_WEEK_FOR_MARK {
            self.total_marks += 1;
            self.marks_at_current_rank += 1;

            return true;
        }

        false
    }

    pub fn try_update_rank(&mut self, current_rank_id: u64) -> bool {
        if self.rank_id != current_rank_id {
            self.rank_id = current_rank_id;
            self.marks_at_current_rank = 0;
            return true;
        }

        false
    }

    pub fn try_reset_events(&mut self) -> bool {
        if let Some(date) = self.last_event_attended_date {
            if has_date_rolled_over(date) {
                self.events_attended_this_week = 0;
                return true;
            }
        } else {
            self.events_attended_this_week = 0;
        }

        false
    }
}

pub async fn get_profile(user_id: u64) -> Result<Profile> {
    let client = Client::new();
    let response = client
        .get(format!("{MAINFRAME_URL}/profiles/{user_id}"))
        .send()
        .await?;

    let profile = response.json::<Profile>().await?;

    Ok(profile)
}

pub async fn get_num_attendance(user_id: u64) -> Result<u64> {
    let client = Client::new();
    let response = client
        .get(format!("{MAINFRAME_URL}/events/num-attended/{user_id}"))
        .send()
        .await?;

    let count = response.json::<u64>().await?;

    Ok(count)
}

pub async fn get_events_attended(user_id: u64) -> Result<Vec<u64>> {
    let client = Client::new();
    let response = client
        .get(format!("{MAINFRAME_URL}/events/attended/{user_id}"))
        .send()
        .await?;

    let events = response.json::<Vec<u64>>().await?;

    Ok(events)
}

pub async fn get_event(event_id: u64) -> Result<Event> {
    let client = Client::new();
    let response = client
        .get(format!("{MAINFRAME_URL}/events/info/{event_id}"))
        .send()
        .await?;

    let event = response.json::<Event>().await?;

    Ok(event)
}

// host's roblox user id, list of roblox usernames
pub async fn log_event(
    host: u64,
    attendees: Vec<String>,
    location: String,
    kind: String,
) -> Result<()> {
    let body = EventJsonBody {
        host,
        names: attendees,
        location,
        kind,
        metadata: None,
    };

    println!("Sending LogEvent {body:?}");

    let client = Client::new();
    client
        .put(format!("{MAINFRAME_URL}/events"))
        .json(&body)
        .send()
        .await?;

    Ok(())
}

pub async fn increment_events(user_id: u64, increment: i32) -> Result<()> {
    let client = Client::new();
    client
        .post(format!(
            "{MAINFRAME_URL}/profiles/increment/{user_id}/{increment}"
        ))
        .send()
        .await?;

    Ok(())
}

#[derive(Deserialize, Serialize)]
pub struct CreateProfileBody {
    pub user_id: u64,
    pub username: String,
    pub rank_id: u64,
    pub events: i32,
    pub marks: i32,
}

pub async fn create_profile(
    user_id: u64,
    username: String,
    rank_id: u64,
    events: i32,
    marks: i32,
) -> Result<()> {
    let body = CreateProfileBody {
        user_id,
        username,
        rank_id,
        events,
        marks,
    };
    let client = Client::new();
    let _ = client
        .post(format!("{MAINFRAME_URL}/profiles/create"))
        .json(&body)
        .send()
        .await?;
    Ok(())
}

pub async fn get_promotable() -> Result<Vec<u64>> {
    let client = Client::new();
    let response = client
        .get(format!("{MAINFRAME_URL}/profiles/promotable"))
        .send()
        .await?;

    let users = response.json::<Vec<u64>>().await?;
    Ok(users)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_date_rollover() {
        let current_time = Utc::now();
        let t1 = current_time - Duration::days(6);
        let t2 = current_time - Duration::days(5);
        assert!(has_date_rolled_over(t1));
        assert!(!has_date_rolled_over(t2));
    }
}