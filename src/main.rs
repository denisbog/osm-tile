use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    fs::File,
    io::{BufReader, BufWriter},
    time::Instant,
};

use cairo::{Context, ImageSurface};
use ciborium::from_reader;
use serde::{Deserialize, Serialize};

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
pub struct Osm {
    pub node: Vec<Node>,
    pub way: Vec<Way>,
}
fn build_index() {
    let start = Instant::now();
    let osm_path = "moldova-latest.osm";
    let buffer = BufReader::new(File::open(osm_path).unwrap());
    let osm: Osm = quick_xml::de::from_reader(buffer).unwrap();
    println!("nodes: {}", osm.node.len());
    println!("ways: {}", osm.way.len());
    println!("loading from osm: {:?}", start.elapsed());

    let writer = BufWriter::new(File::create("test.bin").unwrap());
    ciborium::ser::into_writer(&osm, writer).unwrap();

    let start = Instant::now();
    let mut osm: Osm = from_reader(BufReader::new(File::open("test.bin").unwrap())).unwrap();
    println!("nodes: {}", osm.node.len());
    println!("ways: {}", osm.way.len());

    let mut filters = HashMap::<String, HashSet<String>>::new();
    filters.insert(
        "highway".to_string(),
        HashSet::from_iter(
            vec![
                "primary",
                "secondary",
                "trunk",
                "motorway",
                "primary_link",
                "tertiary",
                "residential",
                "service",
                "unclassified",
            ]
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<String>>(),
        ),
    );

    osm.way = filter(osm.way, &filters);
    println!("ways: {}", osm.way.len());

    let items: HashSet<u64> = osm
        .way
        .iter()
        .flat_map(|item| item.nd.iter().map(|item| item.reference))
        .collect();

    osm.node = osm
        .node
        .into_iter()
        .filter(|item| items.contains(&item.id))
        .collect();

    let writer = BufWriter::new(File::create("test-filter.bin").unwrap());
    ciborium::ser::into_writer(&osm, writer).unwrap();
    println!("loading from binary: {:?}", start.elapsed());

    let start = Instant::now();
    let osm: Osm = from_reader(BufReader::new(File::open("test-filter.bin").unwrap())).unwrap();
    println!("nodes: {}", osm.node.len());
    println!("ways: {}", osm.way.len());

    let mapped: HashMap<u64, (f64, f64)> =
        osm.node
            .iter()
            .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
                acc.insert(item.id, convert_to_tile(item.lat, item.lon, 12));
                acc
            });

    let ways_to_tiles = osm.way.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, way| {
            way.nd.iter().for_each(|node| {
                let tile = mapped.get(&node.reference).unwrap();
                let tile_x = (tile.0 / TILE_SIZE as f64) as i32;
                let tile_y = (tile.1 / TILE_SIZE as f64) as i32;
                acc.entry(tile_x)
                    .or_insert(HashMap::new())
                    .entry(tile_y)
                    .or_insert(HashSet::new())
                    .insert(way.id);
            });
            acc
        },
    );

    let x_s: Vec<f64> = mapped.iter().map(|a| a.1 .0).collect();
    let min_x = x_s.iter().max_by(|a, b| b.total_cmp(&a)).unwrap();
    let max_x = x_s.iter().max_by(|a, b| a.total_cmp(&b)).unwrap();

    let y_s: Vec<f64> = mapped.iter().map(|a| a.1 .1).collect();
    let min_y = y_s.iter().max_by(|a, b| b.total_cmp(&a)).unwrap();
    let max_y = y_s.iter().max_by(|a, b| a.total_cmp(&b)).unwrap();

    println!(
        "coordinated: {:?} {:?} {:?} {:?}",
        min_x, min_y, max_x, max_y
    );
    println!("loading from binary: {:?}", start.elapsed());

    let lat = 47.0245374;
    let long = 28.8406618;
    let tile = convert_to_tile(lat, long, 12);

    let tile = convert_to_int_tile(tile.0, tile.1);

    let ways_id_to_render = ways_to_tiles.get(&tile.0).unwrap().get(&tile.1).unwrap();

    draw_to_image(
        &mapped,
        tile.0 as f64 * TILE_SIZE as f64,
        tile.1 as f64 * TILE_SIZE as f64,
        &osm.way
            .into_iter()
            .filter(|way| ways_id_to_render.contains(&way.id))
            .collect(),
    );
}

fn convert_to_int_tile(lat: f64, lon: f64) -> (i32, i32) {
    let tile_x = (lat / TILE_SIZE as f64) as i32;
    let tile_y = (lon / TILE_SIZE as f64) as i32;
    (tile_x, tile_y)
}

fn draw_to_image(mapped_nodes: &HashMap<u64, (f64, f64)>, min_x: f64, min_y: f64, way: &Vec<Way>) {
    let surface =
        ImageSurface::create(cairo::Format::Rgb24, TILE_SIZE as i32, TILE_SIZE as i32).unwrap();
    let context = Context::new(&surface).unwrap();
    context.set_source_rgb(0.2, 0.2, 0.2);
    context.paint().unwrap();

    context.set_line_width(1f64);
    context.set_line_join(cairo::LineJoin::Round);
    context.set_source_rgb(0.5, 0.5, 0.5);

    way.iter().for_each(|way| {
        way.nd.iter().for_each(|node| {
            let point = mapped_nodes.get(&node.reference).unwrap();

            let x = point.0 - min_x;
            let y = point.1 - min_y;

            // println!("draw line {} {}", x, y);
            context.line_to(x, y);
        });
        // println!("done drawing line");
        context.stroke().unwrap();
    });

    let mut file = File::create("image-tile.png").unwrap();
    surface.write_to_png(&mut file).unwrap();
}
fn filter(way: Vec<Way>, filter: &HashMap<String, HashSet<String>>) -> Vec<Way> {
    way.into_iter()
        .filter(|item| {
            if let Some(tag) = &item.tag {
                tag.iter()
                    .filter(|item| {
                        filter.get(&item.k).is_some()
                            && filter.get(&item.k).unwrap().contains(&item.v)
                    })
                    .count()
                    > 0usize
            } else {
                false
            }
        })
        .collect()
}

fn main() {
    build_index();
}
const TILE_SIZE: u32 = 256;

fn convert_to_tile(lat: f64, lon: f64, zoom: u8) -> (f64, f64) {
    let (lat_rad, lon_rad) = (lat.to_radians(), lon.to_radians());
    let x = lon_rad + PI;
    let y = PI - ((PI / 4f64) + (lat_rad / 2f64)).tan().ln();

    let rescale = |x: f64| {
        let factor = x / (2f64 * PI);
        let dimension_in_pixels = f64::from(TILE_SIZE * (1 << zoom));
        factor * dimension_in_pixels
    };
    (rescale(x), rescale(y))
}
