pub mod utils;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const TILE_SIZE: u32 = 256;

#[derive(Deserialize, Serialize)]
pub struct Node {
    #[serde(rename = "@id")]
    pub id: u64,
    #[serde(rename = "@lat")]
    pub lat: f64,
    #[serde(rename = "@lon")]
    pub lon: f64,
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Nd {
    #[serde(rename = "@ref")]
    pub reference: u64,
}
#[derive(Deserialize, Serialize)]
pub struct Tag {
    #[serde(rename = "@k")]
    pub k: String,
    #[serde(rename = "@v")]
    pub v: String,
}
#[derive(Deserialize, Serialize)]
pub struct Way {
    #[serde(rename = "@id")]
    pub id: u64,
    pub nd: Vec<Nd>,
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Member {
    #[serde(rename = "@type")]
    pub member_type: String,
    #[serde(rename = "@ref")]
    pub member_ref: u64,
    #[serde(rename = "@role")]
    pub role: String,
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Relation {
    #[serde(rename = "@id")]
    pub id: u64,
    pub member: Vec<Member>,
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Osm {
    pub node: Vec<Arc<Node>>,
    pub way: Vec<Arc<Way>>,
    pub relation: Vec<Arc<Relation>>,
}
