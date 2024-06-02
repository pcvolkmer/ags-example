use std::collections::HashMap;

use askama::Template;
use axum::{Json, Router};
use axum::extract::Query;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use csv::{ReaderBuilder, StringRecord};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;

static AGS_CSV: &str = include_str!("resources/ags.csv");

#[derive(Serialize, Deserialize)]
struct Entry {
    gemeindeschluessel: String,
    kreisschluessel: String,
    kreisfrei: bool,
    plz: String,
    ort: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    kreis: String,
    bundesland: String,
    similarity: u8
}

impl Entry {
    fn from_record(record: StringRecord) -> Entry {
        Self {
            gemeindeschluessel: record.get(0).unwrap().to_string(),
            kreisschluessel: record.get(0).unwrap()[0..5].to_string(),
            kreisfrei: record.get(0).unwrap()[5..8].to_string() == "000",
            plz: record.get(1).unwrap().to_string(),
            ort: record.get(2).unwrap().to_string(),
            kreis: record.get(3).unwrap().to_string(),
            bundesland: record.get(4).unwrap().to_string(),
            similarity: 100
        }
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    query: String,
    multiple_assigned: Vec<String>,
    entries: Vec<Entry>
}

fn find_entries(query: String) -> Vec<Entry> {
    let query = query.trim();

    if query.is_empty() {
        return vec![]
    }

    ReaderBuilder::new().from_reader(AGS_CSV.as_bytes())
        .records()
        .filter(|record| record.is_ok())
        .map(|record| record.unwrap())
        .map(|record| Entry::from_record(record))
        .map(|mut entry| {
            if format!("{} {}", entry.plz, entry.ort.to_lowercase()).starts_with(&query.to_lowercase()) {
                entry.similarity = 100
            } else {
                entry.similarity = (100.0 * jaro_winkler(&query.to_lowercase(), &entry.ort.to_lowercase())) as u8
            }
            entry
        })
        .filter(|entry| entry.similarity > 90)
        .sorted_by(|e1, e2| e2.similarity.cmp(&e1.similarity))
        .take(25)
        .collect::<Vec<Entry>>()
}

fn find_multiple_assigned_zips() -> Vec<String> {
    ReaderBuilder::new().from_reader(AGS_CSV.as_bytes())
        .records()
        .filter(|record| record.is_ok())
        .map(|record| record.unwrap())
        .map(|record| Entry::from_record(record))
        .map(|entry| (entry.plz.to_string(), entry.kreisschluessel.to_string()))
        .sorted_by(|e1, e2| e1.0.cmp(&e2.0))
        .chunk_by(|entry| entry.0.to_string())
        .into_iter()
        .map(|(zip, entries)| (zip, entries.unique().collect_vec()))
        .filter(|(_, entries)| entries.len() > 1 )
        .into_iter()
        .map(|(a, _)| a)
        .unique()
        .collect::<Vec<_>>()
}

async fn api_search(query: Query<HashMap<String, String>>) -> Response {
    if query.get("ma").unwrap_or(&String::new()).to_string() == "1" {
        return Json::from(find_multiple_assigned_zips()).into_response()
    }

    let query = query.get("q").unwrap_or(&String::new()).to_string();
    Json::from(find_entries(query)).into_response()
}

async fn index(query: Query<HashMap<String, String>>) -> IndexTemplate {
    let multiple_assigned = if query.get("ma").unwrap_or(&String::new()).to_string() == "1" {
       find_multiple_assigned_zips()
    } else {
        vec![]
    };

    let query = query.get("q").unwrap_or(&String::new()).to_string();
    IndexTemplate {
        query: query.to_string(),
        multiple_assigned,
        entries: find_entries(query.to_string())
    }
}

#[tokio::main]
async fn main() {

    let app = Router::new()
        .route("/", get(index))
        .route("/api", get(api_search));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap()

}
