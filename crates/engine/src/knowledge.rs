//! What the assistant reads, from the open library and its map.

use crate::ai::assistant::Knowledge;
use crate::library::Library;
use crate::map::{self, Map};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct EngineKnowledge {
    pub library: Arc<Library>,
    pub map: Arc<Map>,
}

impl Knowledge for EngineKnowledge {
    fn search(&self, q: &str, k: usize, kind: &str, lang: &str) -> Vec<Value> {
        self.library.search(q, k, kind, lang)
    }

    fn record(&self, id: &str) -> Option<Value> {
        let cat = self.library.catalog();
        let r = cat.get(id)?;
        let mut v = Value::Object(r.raw.clone());
        v["kind"] = json!(r.kind);
        v["title"] = json!(r.title);
        Some(v)
    }

    fn public(&self, id: &str) -> Option<Value> {
        let cat = self.library.catalog();
        cat.get(id).map(|r| self.library.public(r, false))
    }

    fn page_texts(&self, id: &str) -> Vec<String> {
        let cat = self.library.catalog();
        cat.get(id).map(|r| self.library.page_texts(r)).unwrap_or_default()
    }

    fn transcript(&self, id: &str) -> Option<Value> {
        self.library.transcript(id)
    }

    fn topics(&self) -> Vec<Value> {
        self.library.topics()
    }

    fn places(&self, q: &str, limit: usize) -> Vec<Value> {
        self.map.places(q, limit, None)
    }

    fn near(&self, lat: f64, lon: f64, kinds: &[String], limit: usize) -> Vec<Value> {
        let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
        self.map.near(lat, lon, &kinds, limit)
    }

    fn help_kinds(&self) -> Vec<String> {
        map::HELP.iter().map(|(k, _)| k.to_string()).collect()
    }

    /// The map's source and date, as its files name them (Map::data_credit).
    fn map_credit(&self) -> Value {
        self.map.data_credit()
    }
}
