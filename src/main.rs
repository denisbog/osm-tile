use axum::{extract::Path, http::header, routing::get, Extension, Router};
use cairo::{Context, ImageSurface};
use osm_tiles::{
    utils::{convert_to_int_tile, convert_to_tile, creat_filter, filter},
    Osm, Way, TILE_SIZE,
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, BufWriter},
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::Mutex;

type WayToTile = HashMap<i32, HashMap<i32, HashSet<u64>>>;
type NodeToTile = HashMap<u64, (f64, f64)>;

const OSM_PATH: &str = "moldova-latest.osm";

fn build_index_for_zoom(osm: &Osm, zoom: u8) -> (WayToTile, NodeToTile) {
    println!("build new cache for zoom {}", zoom);
    let nodes_to_tile: NodeToTile =
        osm.node
            .iter()
            .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
                acc.insert(item.id, convert_to_tile(item.lat, item.lon));
                acc
            });

    let dimension_in_pixels_for_zoom = f64::from(TILE_SIZE * (1 << zoom));

    let nodes_to_tile: NodeToTile = nodes_to_tile
        .into_iter()
        .map(|(id, (x, y))| {
            (
                id,
                (
                    x * dimension_in_pixels_for_zoom,
                    y * dimension_in_pixels_for_zoom,
                ),
            )
        })
        .collect();

    let ways_to_tiles = osm.way.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, way| {
            way.nd.iter().for_each(|node| {
                let tile = nodes_to_tile.get(&node.reference).unwrap();
                let tile = convert_to_int_tile(tile.0, tile.1);
                acc.entry(tile.0)
                    .or_insert(HashMap::new())
                    .entry(tile.1)
                    .or_insert(HashSet::new())
                    .insert(way.id);
            });
            acc
        },
    );

    (ways_to_tiles, nodes_to_tile)
}

fn filter_osm() -> Osm {
    let buffer = BufReader::new(File::open(OSM_PATH).unwrap());
    let mut osm: Osm = quick_xml::de::from_reader(buffer).unwrap();

    osm.way = filter(osm.way, &creat_filter());

    let nodes_relevant_to_filtered_ways: HashSet<u64> = osm
        .way
        .iter()
        .flat_map(|item| item.nd.iter().map(|item| item.reference))
        .collect();

    osm.node
        .retain(|item| nodes_relevant_to_filtered_ways.contains(&item.id));
    osm
}

async fn render_tile_inner(
    x: i32,
    y: i32,
    ways_to_tiles: &WayToTile,
    mapped: &NodeToTile,
    ways: &Vec<Way>,
) -> Vec<u8> {
    let filtered_ways = if let Some(inner) = ways_to_tiles.get(&x) {
        if let Some(inner) = inner.get(&y) {
            ways.iter().filter(|way| inner.contains(&way.id)).collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    draw_to_memory(
        &mapped,
        x as f64 * TILE_SIZE as f64,
        y as f64 * TILE_SIZE as f64,
        &filtered_ways,
    )
}

struct TileCache {
    osm: Osm,
    cache: HashMap<u8, (WayToTile, NodeToTile)>,
}

impl TileCache {
    fn new(osm: Osm) -> Self {
        TileCache {
            osm,
            cache: HashMap::new(),
        }
    }
    fn get_cache(&mut self, zoom: u8) -> (&Vec<Way>, &WayToTile, &NodeToTile) {
        let cache = self
            .cache
            .entry(zoom)
            .or_insert_with_key(|&zoom| build_index_for_zoom(&self.osm, zoom));
        (&self.osm.way, &cache.0, &cache.1)
    }
}

async fn render_tile(
    Path((z, x, y)): Path<(i32, i32, i32)>,
    Extension(tile_cache): Extension<Arc<Mutex<TileCache>>>,
) -> impl axum::response::IntoResponse {
    let mut lock = tile_cache.lock().await;
    let temp = lock.get_cache(z as u8);
    (
        axum::response::AppendHeaders([(header::CONTENT_TYPE, "image/png")]),
        render_tile_inner(x, y, &temp.1, &temp.2, temp.0).await,
    )
}

fn draw_to_memory(
    mapped_nodes: &HashMap<u64, (f64, f64)>,
    min_x: f64,
    min_y: f64,
    way: &[&Way],
) -> Vec<u8> {
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

            context.line_to(x, y);
        });
        context.stroke().unwrap();
    });
    let mut buffer = BufWriter::new(Vec::<u8>::new());
    surface.write_to_png(&mut buffer).unwrap();
    buffer.into_inner().unwrap()
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/:z/:x/:y", get(render_tile))
        .layer(Extension(Arc::new(Mutex::new(
            TileCache::new(filter_osm()),
        ))));
    axum::Server::bind(&SocketAddr::from(([0, 0, 0, 0], 4000)))
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[cfg(test)]
mod test {
    // use std::{
    //     fs::File,
    //     io::{BufWriter, Write},
    //     sync::Arc,
    // };

    // use crate::{build_index_for_zoom, filter_osm, render_tile_inner};

    #[tokio::test]
    async fn name() {
        // let osm = filter_osm();
        // let index = build_index_for_zoom(osm, 14);
        // let data = render_tile_inner(
        //     297,
        //     178,
        //     Arc::new(index.2),
        //     Arc::new(index.1),
        //     Arc::new(index.0),
        // )
        // .await;
        //
        // BufWriter::new(File::create("test-tile.png").unwrap())
        //     .write(&data)
        //     .unwrap();
    }
}
