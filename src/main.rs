use std::collections::HashMap;
use askama::Template;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use axum::extract::Query;
use axum::routing::get;
use csv::{ReaderBuilder, StringRecord};
use serde::{Deserialize, Serialize};

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
    bundesland: String
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
        }
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    query: String,
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
        .filter(|entry|
            entry.plz.starts_with(&query)
                || entry.ort.starts_with(&query)
                || format!("{} {}", entry.plz, entry.ort).starts_with(&query)

        )
        .take(25)
        .collect::<Vec<Entry>>()
}

async fn api_search(query: Query<HashMap<String, String>>) -> Response {
    let query = query.get("q").unwrap_or(&String::new()).to_string();
    Json::from(find_entries(query)).into_response()
}

async fn index(query: Query<HashMap<String, String>>) -> IndexTemplate {
    let query = query.get("q").unwrap_or(&String::new()).to_string();
    IndexTemplate {
        query: query.to_string(),
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
