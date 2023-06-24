use std::{
    fs::File,
    io::{BufReader, BufWriter},
};

use ciborium::into_writer;
use osm_tiles::Osm;

const OSM_PATH: &str = "moldova-latest.osm";

fn main() {
    let buffer = BufReader::new(File::open(OSM_PATH).unwrap());
    let osm: Osm = quick_xml::de::from_reader(buffer).unwrap();
    // osm.way = filter(osm.way, &creat_filter());
    //
    // let nodes_relevant_to_filtered_ways: HashSet<u64> = osm
    //     .way
    //     .iter()
    //     .flat_map(|item| item.nd.iter().map(|item| item.reference))
    //     .collect();
    // osm.node
    //     .retain(|item| nodes_relevant_to_filtered_ways.contains(&item.id));

    into_writer(&osm, BufWriter::new(File::create("osm.bin").unwrap())).unwrap();
}
