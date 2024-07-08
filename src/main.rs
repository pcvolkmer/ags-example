use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use askama::Template;
use axum::{Json, Router};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use csv::{ReaderBuilder, StringRecord};
use itertools::Itertools;
use lazy_static::lazy_static;
use moka::future::Cache;
use regex::Regex;
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;

static AGS_CSV: &str = include_str!("resources/ags.csv");

lazy_static! {
    static ref PLZ_RE: Regex = Regex::new(r"^(?<plz>[0-9]{5})(\s+)(?<ort>.+)").unwrap();
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    gemeindeschluessel: String,
    kreisschluessel: String,
    kreisfrei: bool,
    plz: String,
    ort: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    kreis: String,
    bundesland: String,
    similarity: u8,
}

impl Entry {
    fn from_record(record: &StringRecord) -> Entry {
        Self {
            gemeindeschluessel: record.get(0).unwrap().to_string(),
            kreisschluessel: record.get(0).unwrap()[0..5].to_string(),
            kreisfrei: record.get(0).unwrap()[5..8].to_string() == "000",
            plz: record.get(1).unwrap().to_string(),
            ort: record.get(2).unwrap().to_string(),
            kreis: record.get(3).unwrap().to_string(),
            bundesland: record.get(4).unwrap().to_string(),
            similarity: 0,
        }
    }

    fn with_similarity(mut self, similarity: u8) -> Entry {
        self.similarity = similarity;
        self
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    query: String,
    multiple_assigned: BTreeMap<String, Vec<String>>,
    entries: Vec<Entry>,
}

fn all_entries() -> Vec<Entry> {
    ReaderBuilder::new()
        .from_reader(AGS_CSV.as_bytes())
        .records()
        .filter(|record| record.is_ok())
        .map(|record| record.unwrap())
        .map(|record| Entry::from_record(&record))
        .collect_vec()
}

fn get_similarity(query: &str, entry: &Entry) -> u8 {
    if entry.plz.to_lowercase().starts_with(&query)
        || entry.ort.to_lowercase().starts_with(&query)
        || format!("{} {}", entry.plz, entry.ort.to_lowercase()).starts_with(&query)
    {
        100
    } else if match PLZ_RE.captures(&query) {
        Some(caps) => {
            caps["plz"] == entry.plz
                && jaro_winkler(&caps["ort"], &entry.ort.to_lowercase()) >= 0.85
        }
        _ => false,
    } {
        100
    } else if !PLZ_RE.is_match(&query) {
        (100.0 * jaro_winkler(&query, &entry.ort.to_lowercase())) as u8
    } else {
        entry.similarity
    }
}

async fn find_entries(query: String, cache: Cache<String, Vec<Entry>>) -> Vec<Entry> {
    let query = query.trim().to_lowercase();

    if query.is_empty() {
        return vec![];
    }

    if let Some(entries) = cache.get(&query).await {
        return entries;
    }

    let entries = all_entries()
        .into_iter()
        .map(|entry| {
            let similarity = get_similarity(&query, &entry);
            entry.with_similarity(similarity)
        })
        .filter(|entry| entry.similarity >= 90)
        .sorted_by(|e1, e2| e2.similarity.cmp(&e1.similarity))
        .take(25)
        .collect::<Vec<Entry>>();

    cache.insert(query, entries.clone()).await;

    entries
}

fn find_multiple_assigned_zips() -> BTreeMap<String, Vec<String>> {
    all_entries()
        .iter()
        .map(|entry| (entry.plz.to_string(), entry.kreisschluessel.to_string()))
        .sorted_by(|e1, e2| e1.0.cmp(&e2.0))
        .chunk_by(|entry| entry.0.to_string())
        .into_iter()
        .map(|(zip, entries)| (zip, entries.unique().collect_vec()))
        .filter(|(_, entries)| entries.len() > 1)
        .into_iter()
        .map(|(a, _)| a)
        .unique()
        .into_group_map_by(|entry| format!("{}...", entry[0..1].to_string()))
        .into_iter()
        .collect::<BTreeMap<_,_>>()
}

async fn api_search(
    State(cache): State<Cache<String, Vec<Entry>>>,
    query: Query<HashMap<String, String>>,
) -> Response {
    if query.get("ma").unwrap_or(&String::new()).to_string() == "1" {
        return Json::from(find_multiple_assigned_zips()).into_response();
    }

    let query = query.get("q").unwrap_or(&String::new()).trim().to_string();
    Json::from(find_entries(query, cache).await).into_response()
}

async fn index(
    State(cache): State<Cache<String, Vec<Entry>>>,
    query: Query<HashMap<String, String>>,
) -> IndexTemplate {
    let multiple_assigned = if query.get("ma").unwrap_or(&String::new()).to_string() == "1" {
        find_multiple_assigned_zips()
    } else {
        BTreeMap::new()
    };

    let query = query.get("q").unwrap_or(&String::new()).to_string();
    IndexTemplate {
        query: query.trim().to_string(),
        multiple_assigned,
        entries: find_entries(query.to_string(), cache).await,
    }
}

#[tokio::main]
async fn main() {
    let cache: Cache<String, Vec<Entry>> = Cache::builder()
        .max_capacity(1000)
        .time_to_live(Duration::from_secs(30 * 60))
        .time_to_idle(Duration::from_secs(5 * 60))
        .build();

    let app = Router::new()
        .route("/", get(index))
        .route("/api", get(api_search))
        .with_state(cache);

    let listener = tokio::net::TcpListener::bind("[::]:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap()
}
