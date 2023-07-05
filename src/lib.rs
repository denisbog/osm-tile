pub mod utils;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

pub const TILE_SIZE: u32 = 256;

pub type RelationToTile = HashMap<i32, HashMap<i32, HashSet<u64>>>;
pub type WayToTile = HashMap<i32, HashMap<i32, HashSet<u64>>>;
pub type NodeToTile = HashMap<u64, (f64, f64)>;

#[derive(Deserialize, Serialize)]
pub struct Node {
    #[serde(rename = "@id")]
    pub id: u64,
    #[serde(rename = "@lat")]
    pub lat: f64,
    #[serde(rename = "@lon")]
    pub lon: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Relation {
    #[serde(rename = "@id")]
    pub id: u64,
    pub member: Vec<Member>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Vec<Tag>>,
}

#[derive(Deserialize, Serialize)]
pub struct Osm {
    pub relation: Vec<Arc<Relation>>,
    pub way: Vec<Arc<Way>>,
    pub node: Vec<Arc<Node>>,
}
#[derive(PartialEq, Eq, Hash)]
pub enum Type {
    Park,
    Building,
    Generic,
    Water,
    WaterRiver,
}

pub struct LoopWithType {
    pub member_type: Type,
    pub memeber_loop: Vec<u64>,
    pub way_id: Option<u64>,
}

impl LoopWithType {
    pub fn new_no_default() -> Self {
        Self {
            member_type: Type::Generic,
            memeber_loop: Vec::<u64>::new(),
            way_id: None,
        }
    }
    pub fn new_with_type(way_id: u64, memeber_type: Type) -> Self {
        Self {
            member_type: memeber_type,
            memeber_loop: Vec::<u64>::new(),
            way_id: Some(way_id),
        }
    }
}
