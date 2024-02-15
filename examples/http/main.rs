use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use typeshare::typeshare;

mod api;

#[derive(Clone, Debug)]
struct Item {
    id: String,
    content: String,
}

#[derive(Clone, Debug)]
pub struct AppState(Arc<RwLock<HashMap<String, Item>>>);

impl AppState {
    pub fn new() -> Self {
        AppState(Arc::new(RwLock::new(HashMap::new())))
    }

    pub fn insert(&self, id: String, item: Item) -> bool {
        let mut m = self.0.write().unwrap();
        m.insert(id, item).is_none()
    }

    pub fn list(&self) -> Vec<Item> {
        let m = self.0.read().unwrap();
        m.values().cloned().collect()
    }

    pub fn update(&self, id: String, content: String) -> bool {
        let mut m = self.0.write().unwrap();
        if let Entry::Occupied(_) = m.entry(id.to_owned()).and_modify(|e| e.content = content) {
            true
        } else {
            false
        }
        // if m.contains_key(&id) {
        //     m.insert(id, item);
        //     true
        // } else {
        //     false
        // }
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut m = self.0.write().unwrap();
        m.remove(id).is_some()
    }
}

#[typeshare]
#[derive(Serialize, Deserialize)]
pub struct TestStruct {
    #[serde(rename = "0")]
    _0: String,
    #[serde(rename = "1")]
    _1: String,
}

pub fn main() {
    println!("{}", api::v1::mod_path());
}
