use axum::{extract::Path, http::header, routing::get, Extension, Router};
use cairo::{Context, ImageSurface};
use ciborium::from_reader;
use env_logger::Env;
use log::info;
use osm_tiles::{
    utils::{convert_to_int_tile, convert_to_tile},
    Osm, Relation, Way, TILE_SIZE,
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, BufWriter},
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};
use tokio::sync::Mutex;

type WayToTile = HashMap<i32, HashMap<i32, HashSet<u64>>>;
type NodeToTile = HashMap<u64, (f64, f64)>;
type IdToRelation = HashMap<u64, Arc<Relation>>;
type IdToWay = HashMap<u64, Arc<Way>>;
type RelationToTile = HashMap<i32, HashMap<i32, HashSet<u64>>>;

struct Index {
    way_to_tile: WayToTile,
    node_to_tile: NodeToTile,
    relation_to_tile: RelationToTile,
    id_to_relation: IdToRelation,
    id_to_way: IdToWay,
}
fn build_index_for_zoom(osm: Arc<Osm>, zoom: u8) -> Index {
    let start = Instant::now();
    info!("build new cache for zoom {}", zoom);
    let nodes_to_tile: NodeToTile =
        osm.node
            .iter()
            .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
                acc.insert(item.id, convert_to_tile(item.lat, item.lon));
                acc
            });

    let dimension_in_pixels_for_zoom = f64::from(TILE_SIZE * (1 << zoom));

    let node_to_tile: NodeToTile = nodes_to_tile
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

    let way_to_tile = osm.way.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, way| {
            way.nd.iter().for_each(|node| {
                let tile = node_to_tile.get(&node.reference).unwrap();
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

    let id_to_way = osm
        .way
        .iter()
        .fold(HashMap::<u64, Arc<Way>>::new(), |mut acc, item| {
            acc.insert(item.id, item.clone());
            acc
        });
    let id_to_relation =
        osm.relation
            .iter()
            .fold(HashMap::<u64, Arc<Relation>>::new(), |mut acc, relation| {
                acc.insert(relation.id, relation.clone());
                acc
            });

    let relation_to_tile = osm.relation.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, relation| {
            relation.member.iter().for_each(|member| {
                if let Some(way) = id_to_way.get(&member.member_ref) {
                    way.nd.iter().for_each(|node| {
                        let tile = node_to_tile.get(&node.reference).unwrap();
                        let tile = convert_to_int_tile(tile.0, tile.1);
                        acc.entry(tile.0)
                            .or_insert(HashMap::new())
                            .entry(tile.1)
                            .or_insert(HashSet::new())
                            .insert(relation.id);
                    });
                }
            });
            acc
        },
    );

    info!("index build in {:?} for zoom {}", start.elapsed(), zoom);
    Index {
        way_to_tile,
        node_to_tile,
        relation_to_tile,
        id_to_relation,
        id_to_way,
    }
}

fn load_binary_osm() -> Osm {
    from_reader(BufReader::new(File::open("osm.bin").unwrap())).unwrap()
}

async fn render_tile_inner(x: i32, y: i32, osm: Arc<Osm>, index: &Index) -> Vec<u8> {
    let filtered_ways = if let Some(inner) = index.way_to_tile.get(&x) {
        if let Some(inner) = inner.get(&y) {
            osm.way
                .iter()
                .cloned()
                .filter(|way| inner.contains(&way.id))
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    draw_to_memory(
        &index.node_to_tile,
        x as f64 * TILE_SIZE as f64,
        y as f64 * TILE_SIZE as f64,
        &filtered_ways,
    )
}

struct TileCache {
    osm: Arc<Osm>,
    cache: HashMap<u8, Arc<Index>>,
}

impl TileCache {
    fn new(osm: Arc<Osm>) -> Self {
        TileCache {
            osm,
            cache: HashMap::new(),
        }
    }
    fn get_cache(&mut self, zoom: u8) -> &Index {
        let cache = self
            .cache
            .entry(zoom)
            .or_insert_with_key(|&zoom| Arc::new(build_index_for_zoom(self.osm.clone(), zoom)));
        cache
    }
}

async fn render_tile(
    Path((z, x, y)): Path<(i32, i32, i32)>,
    Extension(tile_cache): Extension<Arc<Mutex<TileCache>>>,
    Extension(osm): Extension<Arc<Osm>>,
) -> impl axum::response::IntoResponse {
    let mut lock = tile_cache.lock().await;
    let temp = lock.get_cache(z as u8);
    (
        axum::response::AppendHeaders([(header::CONTENT_TYPE, "image/png")]),
        render_tile_inner(x, y, osm.clone(), temp).await,
    )
}

fn draw_to_memory(
    mapped_nodes: &HashMap<u64, (f64, f64)>,
    min_x: f64,
    min_y: f64,
    way: &Vec<Arc<Way>>,
) -> Vec<u8> {
    let surface =
        ImageSurface::create(cairo::Format::Rgb24, TILE_SIZE as i32, TILE_SIZE as i32).unwrap();
    let context = Context::new(&surface).unwrap();
    context.set_source_rgb(0.2, 0.2, 0.2);
    context.paint().unwrap();

    context.set_line_width(1f64);
    context.set_line_cap(cairo::LineCap::Round);
    context.set_line_join(cairo::LineJoin::Round);
    context.set_source_rgb(0.5, 0.5, 0.5);
    context.set_line_width(1f64);

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
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let osm = Arc::new(load_binary_osm());

    let app = Router::new()
        .route("/:z/:x/:y", get(render_tile))
        .layer(Extension(Arc::new(Mutex::new(TileCache::new(osm.clone())))))
        .layer(Extension(osm.clone()));

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
