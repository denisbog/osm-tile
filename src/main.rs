use axum::{
    extract::Path,
    http::{header, Method},
    routing::get,
    Extension, Router,
};
use cairo::{Context, ImageSurface};
use ciborium::from_reader;
use env_logger::Env;
use log::info;
use osm_tiles::{
    utils::{
        convert_to_int_tile, convert_to_tile, create_filter_expression, filter_relations,
        filter_ways_from_relations,
    },
    NodeToTile, Osm, Relation, RelationToTile, Way, WayToTile, TILE_SIZE,
};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, BufWriter},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
};

struct Index {
    relations_to_tile: RelationToTile,
    ways_to_tile: WayToTile,
    node_to_tile_zoom_coordinates: NodeToTile,
    id_to_ways: HashMap<u64, Arc<Way>>,
}
fn build_index_for_zoom(osm: Arc<Osm>, zoom: u8) -> Index {
    info!("build new cache for zoom {}", zoom);
    let nodes_to_tile: NodeToTile =
        osm.node
            .iter()
            .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
                acc.insert(item.id, convert_to_tile(item.lat, item.lon));
                acc
            });

    let dimension_in_pixels_for_zoom = f64::from(TILE_SIZE * (1 << zoom));

    let node_to_tile_zoom_coordinates: NodeToTile = nodes_to_tile
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

    let sorrund_tiles_window = [1, 1, 0, 1, -1, -1, 0, -1, 1];

    let id_to_ways = osm
        .way
        .iter()
        .fold(HashMap::<u64, Arc<Way>>::new(), |mut acc, way| {
            acc.insert(way.id, way.clone());
            acc
        });

    let relations_to_tile = osm.relation.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, relation| {
            relation
                .member
                .iter()
                .filter(|member| member.role.eq("outer"))
                .flat_map(|member| id_to_ways.get(&member.member_ref))
                .flat_map(|way| way.nd.iter())
                .for_each(|node| {
                    let tile = node_to_tile_zoom_coordinates.get(&node.reference).unwrap();
                    let tile = convert_to_int_tile(tile.0, tile.1);
                    acc.entry(tile.0)
                        .or_insert(HashMap::new())
                        .entry(tile.1)
                        .or_insert(HashSet::new())
                        .insert(relation.id);
                    if zoom > 15 {
                        sorrund_tiles_window.windows(2).for_each(|sliding_window| {
                            acc.entry(tile.0 + sliding_window[0])
                                .or_insert(HashMap::new())
                                .entry(tile.1 + sliding_window[1])
                                .or_insert(HashSet::new())
                                .insert(relation.id);
                        })
                    }
                });
            acc
        },
    );

    let ways_from_relations =
        osm.relation
            .iter()
            .fold(HashSet::<u64>::new(), |mut acc, relation| {
                relation.member.iter().for_each(|member| {
                    acc.insert(member.member_ref);
                });
                acc
            });

    let ways_not_part_of_relation: Vec<Arc<Way>> = osm
        .way
        .iter()
        .cloned()
        .filter(|way| !ways_from_relations.contains(&way.id))
        .collect();

    let ways_to_tile = ways_not_part_of_relation.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, way| {
            way.nd.iter().for_each(|node| {
                let tile = node_to_tile_zoom_coordinates.get(&node.reference).unwrap();
                let tile = convert_to_int_tile(tile.0, tile.1);
                acc.entry(tile.0)
                    .or_insert(HashMap::new())
                    .entry(tile.1)
                    .or_insert(HashSet::new())
                    .insert(way.id);
                if zoom > 15 {
                    sorrund_tiles_window.windows(2).for_each(|sliding_window| {
                        acc.entry(tile.0 + sliding_window[0])
                            .or_insert(HashMap::new())
                            .entry(tile.1 + sliding_window[1])
                            .or_insert(HashSet::new())
                            .insert(way.id);
                    })
                }
            });
            acc
        },
    );

    Index {
        relations_to_tile,
        ways_to_tile,
        node_to_tile_zoom_coordinates,
        id_to_ways,
    }
}

fn load_binary_osm() -> Osm {
    from_reader(BufReader::new(File::open("osm.bin").unwrap())).unwrap()
}

async fn render_tile_inner(z: i32, x: i32, y: i32, osm: Arc<Osm>, index: &Index) -> Vec<u8> {
    let filtered_relations = if let Some(inner) = index.relations_to_tile.get(&x) {
        if let Some(inner) = inner.get(&y) {
            osm.relation
                .iter()
                .cloned()
                .filter(|relation| inner.contains(&relation.id))
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let filtered_ways = if let Some(inner) = index.ways_to_tile.get(&x) {
        if let Some(inner) = inner.get(&y) {
            inner
                .iter()
                .flat_map(|inner| index.id_to_ways.get(&inner))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    draw_to_memory(
        z,
        &index.node_to_tile_zoom_coordinates,
        x as f64 * TILE_SIZE as f64,
        y as f64 * TILE_SIZE as f64,
        &filtered_relations,
        &filtered_ways,
        &index.id_to_ways,
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

async fn render_tile_cache(
    Path((z, x, y)): Path<(i32, i32, i32)>,
    Extension(tile_cache): Extension<Arc<Mutex<TileCache>>>,
    Extension(osm): Extension<Arc<Osm>>,
) -> impl axum::response::IntoResponse {
    let new_path = format!("./cached/{}/{}/{}.png", z, x, y);
    let cached = PathBuf::from(&new_path);
    let response = if !cached.is_file() {
        let mut lock = tile_cache.lock().await;
        let temp = lock.get_cache(z as u8);

        let rendered_image = render_tile_inner(z, x, y, osm.clone(), temp).await;

        let last_index = new_path.rfind('/').unwrap();
        tokio::fs::create_dir_all(&new_path[..last_index])
            .await
            .expect("failed to create the directory");
        tokio::fs::write(&new_path, &rendered_image)
            .await
            .expect("storing rendition file");
        rendered_image
    } else {
        tokio::fs::read(&new_path).await.unwrap()
    };
    (
        axum::response::AppendHeaders([
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "max-age=604800"),
        ]),
        response,
    )
}

fn check_if_relation_is_park(relation: &Relation) -> bool {
    if let Some(tag) = &relation.tag {
        if let Some(tag) = tag.iter().find(|t| t.k.eq("leisure")) {
            return tag.v.eq("park");
        }
    }
    false
}
fn check_if_park(way: &Way) -> bool {
    if let Some(tag) = &way.tag {
        if let Some(tag) = tag.iter().find(|t| t.k.eq("leisure")) {
            return tag.v.eq("park");
        }
    }

    false
}
fn draw_to_memory(
    z: i32,
    mapped_nodes: &HashMap<u64, (f64, f64)>,
    min_x: f64,
    min_y: f64,
    relations: &[Arc<Relation>],
    ways: &[Arc<Way>],
    id_to_ways: &HashMap<u64, Arc<Way>>,
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

    let mut last: Option<u64> = None;

    ways.iter().for_each(|way| {
        let is_park = check_if_park(way);

        if is_park {
            context.set_source_rgba(0.5, 1.0, 0.5, 0.2);
        } else {
            context.set_source_rgb(0.5, 0.5, 0.5);
        }
        let reverse = if let Some(last_item) = last {
            if last_item.eq(&way.nd.last().unwrap().reference) {
                false
            } else {
                true
            }
        } else {
            false
        };
        if reverse {
            way.nd.iter().for_each(|node| {
                let point = mapped_nodes.get(&node.reference).unwrap();

                let x = point.0 - min_x;
                let y = point.1 - min_y;

                context.line_to(x, y);
            });
            last = Some(way.nd.last().unwrap().reference);
        } else {
            way.nd.iter().for_each(|node| {
                let point = mapped_nodes.get(&node.reference).unwrap();

                let x = point.0 - min_x;
                let y = point.1 - min_y;

                context.line_to(x, y);
            });
            last = Some(way.nd.first().unwrap().reference);
        };

        if is_park {
            context.fill().unwrap();
        }
        context.stroke().unwrap();
    });

    relations.iter().for_each(|relation| {
        if check_if_relation_is_park(relation) {
            context.set_source_rgba(0.5, 1.0, 0.5, 0.2);
            let ways: Vec<&Arc<Way>> = relation
                .member
                .iter()
                .flat_map(|member| id_to_ways.get(&member.member_ref))
                .collect();

            ways.iter().fold(HashSet::<u64>::new(), |mut acc, way| {
                let reverse = if let Some(last_item) = last {
                    if last_item.eq(&way.nd.last().unwrap().reference) {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                info!("is reverse : {}", reverse);
                info!("last {:?} ", last);
                if reverse {
                    way.nd.iter().for_each(|node| {
                        let point = mapped_nodes.get(&node.reference).unwrap();

                        let x = point.0 - min_x;
                        let y = point.1 - min_y;

                        context.line_to(x, y);
                    });
                    last = Some(way.nd.last().unwrap().reference);
                    info!("now {:?} ", last);
                } else {
                    way.nd.iter().for_each(|node| {
                        let point = mapped_nodes.get(&node.reference).unwrap();

                        let x = point.0 - min_x;
                        let y = point.1 - min_y;

                        context.line_to(x, y);
                    });
                    last = Some(way.nd.first().unwrap().reference);
                };

                info!("now {:?} ", last);

                if acc.contains(&last.unwrap()) {
                    info!("contains last");
                    context.fill().unwrap();
                    context.stroke().unwrap();
                    acc.clear();
                    last = None;
                } else {
                    acc.insert(last.unwrap());
                }

                acc
            });
            context.fill().unwrap();
            context.stroke().unwrap();

            context.set_source_rgb(0.5, 1.0, 0.5);
            let ways: Vec<&Arc<Way>> = relation
                .member
                .iter()
                .flat_map(|member| id_to_ways.get(&member.member_ref))
                .collect();

            ways.iter().enumerate().for_each(|(index, way)| {
                way.nd.iter().enumerate().for_each(|(index_y, node)| {
                    let point = mapped_nodes.get(&node.reference).unwrap();
                    let x = point.0 - min_x;
                    let y = point.1 - min_y;
                    context.rectangle(x - 1f64, y - 1f64, 3f64, 3f64);

                    context.move_to(x, y);
                    context
                        .show_text(&format!("{}-{}", index, index_y))
                        .unwrap();
                });
            });
            context.stroke().unwrap();
        } else {
            context.set_source_rgb(0.5, 0.5, 0.5);
            relation
                .member
                .iter()
                .flat_map(|member| id_to_ways.get(&member.member_ref))
                .for_each(|way| {
                    way.nd.iter().for_each(|node| {
                        let point = mapped_nodes.get(&node.reference).unwrap();

                        let x = point.0 - min_x;
                        let y = point.1 - min_y;

                        context.line_to(x, y);
                    });
                    context.stroke().unwrap();
                });
        }
    });

    // if z > 16 {
    //     ways.iter().for_each(|way| {
    //         way.nd.iter().for_each(|node| {
    //             let point = mapped_nodes.get(&node.reference).unwrap();
    //             let x = point.0 - min_x;
    //             let y = point.1 - min_y;
    //             context.rectangle(x - 1f64, y - 1f64, 3f64, 3f64);
    //         });
    //     });
    //     context.stroke().unwrap();
    // }

    context.set_source_rgb(0.7, 0.7, 0.7);
    context.line_to(TILE_SIZE as f64, 0 as f64);
    context.line_to(0 as f64, 0 as f64);
    context.line_to(0 as f64, TILE_SIZE as f64);
    context.stroke().unwrap();

    let mut buffer = BufWriter::new(Vec::<u8>::new());
    surface.write_to_png(&mut buffer).unwrap();
    buffer.into_inner().unwrap()
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let osm = Arc::new(load_binary_osm());

    let filtered_relations = filter_relations(osm.as_ref(), &create_filter_expression());
    let filtered_ways = filter_ways_from_relations(osm.as_ref(), &filtered_relations);

    let nodes_to_filder: HashSet<u64> = filtered_ways
        .iter()
        .flat_map(|way| way.nd.iter())
        .map(|nd| nd.reference)
        .collect();

    let mut filtered_nodes = osm.node.clone();
    filtered_nodes.retain(|node| nodes_to_filder.contains(&node.id));

    let nodes_to_filder: HashSet<u64> = filtered_ways
        .iter()
        .flat_map(|way| way.nd.iter().map(|nd| nd.reference))
        .collect();

    let mut filtered_nodes = osm.node.clone();
    filtered_nodes.retain(|node| nodes_to_filder.contains(&node.id));

    let filtered_osm = Arc::new(Osm {
        way: filtered_ways,
        node: filtered_nodes,
        relation: filtered_relations,
    });

    // let mut string = String::new();
    // to_writer(&mut string, &filtered_osm).unwrap();
    // let mut buf_writer = BufWriter::new(File::create("temp.xml").unwrap());
    // buf_writer.write_all(string.as_bytes()).unwrap();

    // let filtered_osm = osm.clone();

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = Router::new()
        .nest_service("/", ServeDir::new("../solid-leaflet-reprex/dist"))
        .route("/map/:z/:x/:y", get(render_tile_cache))
        .layer(Extension(Arc::new(Mutex::new(TileCache::new(
            filtered_osm.clone(),
        )))))
        .layer(Extension(filtered_osm.clone()))
        .layer(cors);

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
    #[test]
    fn check() {
        let init = [1, 0, -1];

        init.iter()
            .enumerate()
            .flat_map(|(index, item)| init.iter().skip(index));
    }
}
