pub mod ags {
    use csv::{ReaderBuilder, StringRecord};
    use itertools::Itertools;
    use serde::{Deserialize, Serialize};

    pub static AGS_CSV: &str = include_str!("resources/ags.csv");

    pub fn all_entries() -> Vec<Entry> {
        ReaderBuilder::new()
            .from_reader(AGS_CSV.as_bytes())
            .records()
            .flatten()
            .map(|record| Entry::from_record(&record))
            .collect_vec()
    }

    #[derive(Serialize, Deserialize, Clone)]
    pub struct Entry {
        pub gemeindeschluessel: String,
        pub kreisschluessel: String,
        pub kreisfrei: bool,
        pub plz: String,
        pub ort: String,
        pub lat: String,
        pub lon: String,
        #[serde(skip_serializing_if = "String::is_empty")]
        pub kreis: String,
        pub kreis_lat: String,
        pub kreis_lon: String,
        pub bundesland: String,
        pub deprecated: bool,
        pub einwohner: Option<String>,
        pub similarity: u8,
        pub zip_collision: bool,
        pub primary_zip: bool,
    }

    impl Entry {
        pub fn from_record(record: &StringRecord) -> Entry {
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
                    value => Some(value.to_string()),
                },
                similarity: 0,
                zip_collision: false,
                primary_zip: record.get(11).unwrap_or("0") == "1"
            }
        }

        pub fn with_similarity(mut self, similarity: u8) -> Entry {
            self.similarity = similarity;
            self
        }

        pub fn with_zip_collision(mut self, zip_collision: bool) -> Entry {
            self.zip_collision = zip_collision;
            self
        }

        pub fn get_state_id(&self) -> String {
            self.kreisschluessel[0..2].to_string()
        }
    }

}

