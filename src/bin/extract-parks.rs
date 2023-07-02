use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, BufWriter, Write},
};

use osm_tiles::{
    utils::{filter_relations, filter_ways_from_relations},
    Osm,
};
use quick_xml::se::to_writer;

const OSM_PATH: &str = "moldova-latest.osm";

fn main() {
    let buffer = BufReader::new(File::open(OSM_PATH).unwrap());
    let osm: Osm = quick_xml::de::from_reader(buffer).unwrap();

    let filtered_relations = filter_relations(&osm, &create_filter_expression_to_filter_parks());
    let filtered_ways = filter_ways_from_relations(&osm, &filtered_relations);

    let nodes_to_filder: HashSet<u64> = filtered_ways
        .iter()
        .flat_map(|way| way.nd.iter())
        .map(|nd| nd.reference)
        .collect();

    let mut filtered_nodes = osm.node.clone();
    filtered_nodes.retain(|node| nodes_to_filder.contains(&node.id));

    let filtered_osm = Osm {
        way: filtered_ways,
        node: filtered_nodes,
        relation: filtered_relations,
    };

    let mut string = String::new();
    to_writer(&mut string, &filtered_osm).unwrap();
    let mut buf_writer = BufWriter::new(File::create("temp.xml").unwrap());
    buf_writer.write_all(string.as_bytes()).unwrap();

    // into_writer(
    //     &filtered_osm,
    //     BufWriter::new(File::create("temp.xml").unwrap()),
    // )
    // .unwrap();
}

pub fn create_filter_expression_to_filter_parks() -> HashMap<String, HashSet<String>> {
    let mut filters = HashMap::<String, HashSet<String>>::new();
    filters.insert(
        "addr:city".to_string(),
        HashSet::from_iter(
            vec!["Chișinău"]
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>(),
        ),
    );

    filters.insert(
        "leisure".to_string(),
        HashSet::from_iter(
            vec!["park"]
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>(),
        ),
    );

    // filters.insert(
    //     "wikidata".to_string(),
    //     HashSet::from_iter(
    //         vec!["Q25459177"]
    //             .iter()
    //             .map(|item| item.to_string())
    //             .collect::<Vec<String>>(),
    //     ),
    // );
    // filters.insert(
    //     "name:en".to_string(),
    //     HashSet::from_iter(
    //         vec!["Dendrariu Park"]
    //             .iter()
    //             .map(|item| item.to_string())
    //             .collect::<Vec<String>>(),
    //     ),
    // );

    filters
}
