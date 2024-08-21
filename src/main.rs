use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use askama::Template;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use csv::{ReaderBuilder, StringRecord};
use include_dir::{include_dir, Dir};
use itertools::Itertools;
use lazy_static::lazy_static;
use moka::future::Cache;
use regex::Regex;
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;
#[cfg(debug_assertions)]
use tower_http::trace::TraceLayer;

static AGS_CSV: &str = include_str!("resources/ags.csv");

static GEO_JSON: &str = include_str!("resources/de_small.geojson");

static ASSETS: Dir = include_dir!("src/resources/assets");

lazy_static! {
    static ref PLZ_RE: Regex = Regex::new(r"^(?<plz>[0-9]{5})(\s+)(?<ort>.+)").unwrap();
}

// GeoJSON

#[derive(Serialize, Deserialize, Clone)]
struct GeoJson {
    #[serde(rename="type")]
    type_: String,
    features: Vec<Feature>
}

impl GeoJson {

    fn new() -> GeoJson {
        Self {
            type_: "FeatureCollection".to_string(),
            features: vec![]
        }
    }

    fn all_features() -> Vec<Feature> {
        if let Ok(geo_json) = serde_json::from_str::<GeoJson>(GEO_JSON) {
            return geo_json.features
        }
        vec![]
    }

    fn with_features(mut self, features: Vec<Feature>) -> GeoJson {
        self.features.clear();
        features.iter().for_each(|f| self.features.push(f.clone()));
        self
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Feature {
    id: String,
    geometry: Geometry,
    properties: Properties,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum Geometry {
    Polygon(Polygon),
    MultiPolygon(MultiPolygon)
}

#[derive(Serialize, Deserialize, Clone)]
struct MultiPolygon {
    coordinates: Vec<Vec<Vec<Vec<f32>>>>
}

#[derive(Serialize, Deserialize, Clone)]
struct Polygon {
    coordinates: Vec<Vec<Vec<f32>>>
}

#[derive(Serialize, Deserialize, Clone)]
struct Properties {
    name: String
}

// AGS

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    gemeindeschluessel: String,
    kreisschluessel: String,
    kreisfrei: bool,
    plz: String,
    ort: String,
    lat: String,
    lon: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    kreis: String,
    kreis_lat: String,
    kreis_lon: String,
    bundesland: String,
    deprecated: bool,
    einwohner: Option<String>,
    similarity: u8,
    zip_collision: bool
}

impl Entry {
    fn from_record(record: &StringRecord) -> Entry {
        Self {
            gemeindeschluessel: record.get(0).unwrap().to_string(),
            kreisschluessel: record.get(0).unwrap()[0..5].to_string(),
            kreisfrei: record.get(0).unwrap()[5..8].to_string() == "000",
            plz: record.get(1).unwrap().to_string(),
            ort: record.get(2).unwrap().to_string(),
            lat: record.get(5).unwrap().to_string(),
            lon: record.get(6).unwrap().to_string(),
            kreis: record.get(3).unwrap().to_string(),
            kreis_lat: record.get(7).unwrap().to_string(),
            kreis_lon: record.get(8).unwrap().to_string(),
            bundesland: record.get(4).unwrap().to_string(),
            deprecated: record.get(9).unwrap_or("1") == "1",
            einwohner: match record.get(10).unwrap_or("") {
                "" => None,
                value => Some(value.to_string())
            },
            similarity: 0,
            zip_collision: false
        }
    }

    fn with_similarity(mut self, similarity: u8) -> Entry {
        self.similarity = similarity;
        self
    }

    fn with_zip_collision(mut self, zip_collision: bool) -> Entry {
        self.zip_collision = zip_collision;
        self
    }
    
    fn get_state_id(&self) -> String {
        self.kreisschluessel[0..2].to_string()
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    query: String,
    multiple_assigned: BTreeMap<String, Vec<String>>,
    state: String,
    entries: Vec<Entry>,
}

fn all_entries() -> Vec<Entry> {
    ReaderBuilder::new()
        .from_reader(AGS_CSV.as_bytes())
        .records()
        .flatten()
        .map(|record| Entry::from_record(&record))
        .collect_vec()
}

fn get_similarity(query: &str, entry: &Entry) -> u8 {
    if entry.plz.to_lowercase().starts_with(query)
        || entry.ort.to_lowercase().starts_with(query)
        || format!("{} {}", entry.plz, entry.ort.to_lowercase()).starts_with(query)
    {
        100
    } else if match PLZ_RE.captures(query) {
        Some(caps) => {
            caps["plz"] == entry.plz
                && jaro_winkler(&caps["ort"], &entry.ort.to_lowercase()) >= 0.85
        }
        _ => false,
    } {
        100
    } else if !PLZ_RE.is_match(query) {
        (100.0 * jaro_winkler(query, &entry.ort.to_lowercase())) as u8
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

    let all_multiple_assigned_zips = find_multiple_assigned_zips("")
        .values()
        .flatten()
        .unique()
        .map(|e| e.to_string())
        .collect_vec();
    
    let entries = all_entries()
        .into_iter()
        .map(|entry| {
            let similarity = get_similarity(&query, &entry);
            entry.with_similarity(similarity)
        })
        .map(|entry| {
            let zip_collision = all_multiple_assigned_zips.contains(&entry.plz.to_string());
            entry.with_zip_collision(zip_collision)
        })
        .filter(|entry| entry.similarity >= 90)
        .sorted_by(|e1, e2| e2.similarity.cmp(&e1.similarity))
        .sorted_by(|_, e2|
            if e2.deprecated {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        )
        .take(25)
        .collect::<Vec<Entry>>();

    cache.insert(query, entries.clone()).await;

    entries
}

fn find_multiple_assigned_zips(state: &str) -> BTreeMap<String, Vec<String>> {
    let state = if state.len() > 2 {
        &state[0..2]
    } else {
        state
    };

    let zips_in_state = all_entries()
        .iter()
        .filter(|entry| entry.gemeindeschluessel.starts_with(state))
        .map(|entry| entry.plz.to_string())
        .collect_vec();

    all_entries()
        .iter()
        .map(|entry| (entry.plz.to_string(), entry.kreisschluessel.to_string()))
        .sorted_by(|e1, e2| e1.0.cmp(&e2.0))
        .chunk_by(|entry| entry.0.to_string())
        .into_iter()
        .map(|(zip, entries)| (zip, entries.unique().collect_vec()))
        .filter(|(_, entries)| entries.len() > 1)
        .map(|(a, _)| a)
        .unique()
        .filter(|e| zips_in_state.contains(e))
        .into_group_map_by(|entry| format!("{}...", &entry[0..1]))
        .into_iter()
        .collect::<BTreeMap<_,_>>()
}

fn find_counties_multiple_assigned_zips(state: &str) -> Vec<String> {
    let state = if state.len() > 2 {
        &state[0..2]
    } else {
        state
    };

    let zips_in_state = all_entries()
        .iter()
        .filter(|entry| entry.gemeindeschluessel.starts_with(state))
        .map(|entry| entry.plz.to_string())
        .collect_vec();

    all_entries()
        .iter()
        .map(|entry| (entry.plz.to_string(), entry.kreisschluessel.to_string()))
        .sorted_by(|e1, e2| e1.0.cmp(&e2.0))
        .chunk_by(|entry| entry.0.to_string())
        .into_iter()
        .map(|(zip, entries)| (zip, entries.unique().collect_vec()))
        .filter(|(zip, entries)| zips_in_state.contains(zip) && entries.len() > 1)
        .flat_map(|(_, a)| a.iter().map(|value| value.1.to_string()).collect_vec())
        .unique()
        .collect_vec()
}

async fn api_search(
    State(cache): State<Cache<String, Vec<Entry>>>,
    query: Query<HashMap<String, String>>,
) -> Response {
    if *query.get("ma").unwrap_or(&String::new()) == "1" {
        return Json::from(find_multiple_assigned_zips(query.get("st").unwrap_or(&String::new()))).into_response();
    }

    let query = query.get("q").unwrap_or(&String::new()).trim().to_string();
    Json::from(find_entries(query, cache).await).into_response()
}

async fn geojson(
    query: Query<HashMap<String, String>>,
) -> Response {
    let state = match query.get("st") {
        Some(state) => state.to_string(),
        None => String::new()
    };

    let features = GeoJson::all_features()
        .iter()
        .filter(|&f| f.id.starts_with(&state))
        .cloned()
        .collect_vec();
    
    Json::from(GeoJson::new().with_features(features)).into_response()
}

async fn asg_with_multiple_assigned_zip(
    query: Query<HashMap<String, String>>,
) -> Response {
    let state = match query.get("st") {
        Some(state) => state.to_string(),
        None => String::new()
    };

    Json::from(find_counties_multiple_assigned_zips(&state)).into_response()
}

async fn index(
    State(cache): State<Cache<String, Vec<Entry>>>,
    query: Query<HashMap<String, String>>,
) -> IndexTemplate {
    let state = match query.get("st") {
        Some(state) => state.to_string(),
        None => String::new()
    };
    let multiple_assigned = if *query.get("ma").unwrap_or(&String::new()) == "1" {
        find_multiple_assigned_zips(&state)
    } else {
        BTreeMap::new()
    };

    let query = query.get("q").unwrap_or(&String::new()).to_string();
    IndexTemplate {
        query: query.trim().to_string(),
        multiple_assigned,
        state,
        entries: find_entries(query.to_string(), cache).await,
    }
}

async fn negotiate(
    headers: HeaderMap,
    state_cache: State<Cache<String, Vec<Entry>>>,
    query: Query<HashMap<String, String>>,
) -> impl IntoResponse {
    match headers.get(header::ACCEPT) {
        Some(header) => match header.to_str().unwrap_or_default() {
            "application/json" => api_search(state_cache, query).await.into_response(),
            _ =>  match query.0.get("format") {
                Some(format) if format == "json" => api_search(state_cache, query).await.into_response(),
                _ => index(state_cache, query).await.into_response()
            }
        },
        _ => {
            index(state_cache, query).await.into_response()
        }
    }
}

async fn serve_asset(path: Option<Path<String>>) -> impl IntoResponse {
    match path {
        Some(path) => match ASSETS.get_file(path.to_string()) {
            Some(file) =>  Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(file.contents())),
            None => Response::builder().status(404).body(Body::from("".as_bytes()))
        }
        None => Response::builder().status(400).body(Body::from("".as_bytes()))
    }.unwrap()
}

#[tokio::main]
async fn main() {
    #[cfg(debug_assertions)]
    {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    let cache: Cache<String, Vec<Entry>> = Cache::builder()
        .max_capacity(1000)
        .time_to_live(Duration::from_secs(30 * 60))
        .time_to_idle(Duration::from_secs(5 * 60))
        .build();

    let app = Router::new()
        .route("/", get(negotiate))
        .route("/geojson", get(geojson))
        .route("/counties_mu_zip", get(asg_with_multiple_assigned_zip))
        .route("/api", get(api_search))
        .route("/assets/*path", get(|path| async { serve_asset(path).await }))
        .with_state(cache);

    #[cfg(debug_assertions)]
    let app = app.layer(TraceLayer::new_for_http());
    
    let listener = tokio::net::TcpListener::bind("[::]:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap()
}
