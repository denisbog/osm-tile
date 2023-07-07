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
        check_relation_type, check_way_type, convert_to_int_tile, convert_to_tile,
        extract_loops_to_render, set_context_for_type,
    },
    NodeToTile, Osm, Relation, RelationToTile, Type, Way, WayToTile, TILE_SIZE,
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

fn nodes_to_tile(osm: &Osm) -> NodeToTile {
    osm.node
        .iter()
        .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
            acc.insert(item.id, convert_to_tile(item.lat, item.lon));
            acc
        })
}

struct Index {
    relations_to_tile: RelationToTile,
    ways_to_tile: WayToTile,
    node_to_tile_zoom_coordinates: Arc<NodeToTile>,
    id_to_relations: Arc<HashMap<u64, Arc<Relation>>>,
    id_to_ways: Arc<HashMap<u64, Arc<Way>>>,
    relation_to_type: Arc<HashMap<u64, Type>>,
    way_to_type: Arc<HashMap<u64, Type>>,
}
fn build_index_for_zoom(
    osm: Arc<Osm>,
    nodes_to_tile: Arc<NodeToTile>,
    relation_to_type: Arc<HashMap<u64, Type>>,
    way_to_type: Arc<HashMap<u64, Type>>,
    zoom: u8,
) -> Index {
    info!("build new cache for zoom {}", zoom);

    let dimension_in_pixels_for_zoom = f64::from(TILE_SIZE * (1 << zoom));

    let node_to_tile_zoom_coordinates: NodeToTile = nodes_to_tile
        .iter()
        .map(|(id, (x, y))| {
            (
                id.clone(),
                (
                    x * dimension_in_pixels_for_zoom,
                    y * dimension_in_pixels_for_zoom,
                ),
            )
        })
        .collect();

    let sorrund_tiles_window = [1, 1, 0, 1, -1, -1, 0, -1, 1];

    let id_to_ways = Arc::new(osm.way.iter().fold(
        HashMap::<u64, Arc<Way>>::new(),
        |mut acc, way| {
            acc.insert(way.id, way.clone());
            acc
        },
    ));

    let id_to_relations = Arc::new(osm.relation.iter().fold(
        HashMap::<u64, Arc<Relation>>::new(),
        |mut acc, relation| {
            acc.insert(relation.id, relation.clone());
            acc
        },
    ));

    let relations_to_tile = osm.relation.iter().fold(
        HashMap::<i32, HashMap<i32, HashSet<u64>>>::new(),
        |mut acc, relation| {
            relation
                .member
                .iter()
                // .filter(|member| member.role.eq("outer"))
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
        node_to_tile_zoom_coordinates: Arc::new(node_to_tile_zoom_coordinates),
        id_to_relations,
        id_to_ways,
        relation_to_type,
        way_to_type,
    }
}

fn load_binary_osm() -> Osm {
    from_reader(BufReader::new(File::open("osm.bin").unwrap())).unwrap()
}

async fn render_tile_inner(z: i32, x: i32, y: i32, _osm: Arc<Osm>, index: &Index) -> Vec<u8> {
    let filtered_relations = if let Some(inner) = index.relations_to_tile.get(&x) {
        if let Some(inner) = inner.get(&y) {
            inner.iter().fold(
                HashMap::<Type, Vec<Arc<Relation>>>::new(),
                |mut acc, relation_id| {
                    let relation_type = index.relation_to_type.get(&relation_id).unwrap();
                    let relation = index.id_to_relations.get(&relation_id).unwrap();
                    acc.entry(relation_type.clone())
                        .or_insert(Vec::<Arc<Relation>>::new())
                        .push(relation.clone());
                    acc
                },
            )
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    let filtered_ways = if let Some(inner) = index.ways_to_tile.get(&x) {
        if let Some(inner) = inner.get(&y) {
            inner
                .iter()
                .fold(HashMap::<Type, Vec<Arc<Way>>>::new(), |mut acc, way_id| {
                    let way_type = index.way_to_type.get(&way_id).unwrap();
                    let way = index.id_to_ways.get(&way_id).unwrap();

                    acc.entry(way_type.clone())
                        .or_insert(Vec::<Arc<Way>>::new())
                        .push(way.clone());
                    acc
                })
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    draw_to_memory(
        z,
        index.node_to_tile_zoom_coordinates.clone(),
        x as f64 * TILE_SIZE as f64,
        y as f64 * TILE_SIZE as f64,
        Arc::new(filtered_relations),
        Arc::new(filtered_ways),
        index.id_to_ways.clone(),
    )
}

struct TileCache {
    osm: Arc<Osm>,
    cache: HashMap<u8, Arc<Index>>,
    nodes_to_tile: Arc<NodeToTile>,
    relation_to_type: Arc<HashMap<u64, Type>>,
    way_to_type: Arc<HashMap<u64, Type>>,
}

impl TileCache {
    fn new_no_default(
        osm: Arc<Osm>,
        nodes_to_tile: Arc<NodeToTile>,
        relation_to_type: Arc<HashMap<u64, Type>>,
        way_to_type: Arc<HashMap<u64, Type>>,
    ) -> Self {
        TileCache {
            osm,
            cache: HashMap::new(),
            nodes_to_tile,
            relation_to_type,
            way_to_type,
        }
    }
    fn get_cache(&mut self, zoom: u8) -> &Index {
        let cache = self.cache.entry(zoom).or_insert_with_key(|&zoom| {
            Arc::new(build_index_for_zoom(
                self.osm.clone(),
                self.nodes_to_tile.clone(),
                self.relation_to_type.clone(),
                self.way_to_type.clone(),
                zoom,
            ))
        });
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

fn draw_to_memory(
    z: i32,
    mapped_nodes: Arc<HashMap<u64, (f64, f64)>>,
    min_x: f64,
    min_y: f64,
    relations_for_type: Arc<HashMap<Type, Vec<Arc<Relation>>>>,
    ways_for_type: Arc<HashMap<Type, Vec<Arc<Way>>>>,
    id_to_ways: Arc<HashMap<u64, Arc<Way>>>,
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

    let render_order = [
        Type::Forest,
        Type::Park,
        Type::WaterRiver,
        Type::Water,
        Type::Generic,
        Type::Building,
    ];

    for filter_type in &render_order {
        ways_for_type
            .get(filter_type)
            .unwrap_or(&Vec::<Arc<Way>>::new())
            .iter()
            .for_each(|way| {
                render_way(
                    way,
                    filter_type,
                    &context,
                    mapped_nodes.as_ref(),
                    min_x,
                    min_y,
                    z,
                );
            });

        relations_for_type
            .get(filter_type)
            .unwrap_or(&Vec::<Arc<Relation>>::new())
            .iter()
            .for_each(|relation| {
                render_relation(
                    relation.clone(),
                    filter_type,
                    &context,
                    id_to_ways.clone(),
                    mapped_nodes.clone(),
                    min_x,
                    min_y,
                    z,
                );
            });
    }

    context.set_source_rgb(0.7, 0.7, 0.7);
    context.line_to(TILE_SIZE as f64, 0 as f64);
    context.line_to(0 as f64, 0 as f64);
    context.line_to(0 as f64, TILE_SIZE as f64);
    context.stroke().unwrap();

    let mut buffer = BufWriter::new(Vec::<u8>::new());
    surface.write_to_png(&mut buffer).unwrap();
    buffer.into_inner().unwrap()
}

fn render_relation(
    relation: Arc<Relation>,
    relation_type: &Type,
    context: &Context,
    id_to_ways: Arc<HashMap<u64, Arc<Way>>>,
    mapped_nodes: Arc<HashMap<u64, (f64, f64)>>,
    min_x: f64,
    min_y: f64,
    z: i32,
) {
    set_context_for_type(&relation_type, context);

    let loops = extract_loops_to_render(relation.as_ref(), id_to_ways.as_ref());
    loops.iter().for_each(|ordered_nodes| {
        let way_type = &ordered_nodes.member_type;

        if way_type == &Type::Building {
            context.set_source_rgba(0.5, 0.5, 0.5, 0.2);
        }
        ordered_nodes
            .memeber_loop
            .iter()
            .flat_map(|node| mapped_nodes.get(node))
            .map(|(x, y)| {
                let x = x - min_x;
                let y = y - min_y;
                (x, y)
            })
            .for_each(|(x, y)| {
                context.line_to(x, y);
            });

        if let Type::Forest | Type::Park | Type::Building | Type::Water = way_type {
            context.fill().unwrap();

            if let Type::Building = way_type {
                if z > 16 {
                    //render bulding address
                    if let Some(way_id) = ordered_nodes.way_id {
                        let way = id_to_ways.get(&way_id).unwrap();
                        render_building_number(
                            way,
                            &ordered_nodes.memeber_loop,
                            mapped_nodes.as_ref(),
                            min_x,
                            min_y,
                            context,
                        );
                    }
                }
            }
        } else if let Type::Forest | Type::Park | Type::Water = relation_type {
            context.fill().unwrap();
        }
        context.stroke().unwrap();
    });
    context.stroke().unwrap();
}

fn render_way(
    way: &Arc<Way>,
    way_type: &Type,
    context: &Context,
    mapped_nodes: &HashMap<u64, (f64, f64)>,
    min_x: f64,
    min_y: f64,
    z: i32,
) {
    set_context_for_type(&way_type, context);

    way.nd
        .iter()
        .flat_map(|nd| mapped_nodes.get(&nd.reference))
        .map(|(x, y)| {
            let x = x - min_x;
            let y = y - min_y;
            (x, y)
        })
        .for_each(|(x, y)| {
            context.line_to(x, y);
        });

    if let Type::Forest | Type::Park | Type::Building | Type::Water = way_type {
        if let Type::Building = way_type {
            if z > 16 {
                render_building_number(
                    way,
                    &way.nd.iter().map(|nd| nd.reference).collect::<Vec<u64>>(),
                    mapped_nodes,
                    min_x,
                    min_y,
                    context,
                );
            }
        }
        context.fill().unwrap();
    }
    context.stroke().unwrap();
}

fn render_building_number(
    way: &Way,
    ordered_nodes: &Vec<u64>,
    mapped_nodes: &HashMap<u64, (f64, f64)>,
    min_x: f64,
    min_y: f64,
    context: &Context,
) {
    if let Some(tag) = &way.tag {
        if let Some(tag) = &tag.iter().filter(|tag| tag.k.eq("addr:housenumber")).last() {
            if let Some((x, y)) = ordered_nodes
                .iter()
                .flat_map(|node| mapped_nodes.get(node))
                .map(|(x, y)| {
                    let x = x - min_x;
                    let y = y - min_y;
                    (x, y)
                })
                .reduce(|(x, y), (x1, y1)| (x + x1, y + y1))
                .map(|(x, y)| {
                    let points = ordered_nodes.len() as f64;
                    (x / points, y / points)
                })
            {
                context.move_to(x, y);
            };
            context.show_text(&tag.v).unwrap();
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // let buffer = BufReader::new(File::open("temp.xml").unwrap());
    // let osm: Osm = quick_xml::de::from_reader(buffer).unwrap();

    let osm = Arc::new(load_binary_osm());

    let filtered_osm = osm.clone();
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any)
        .allow_origin(Any);

    let relation_to_type =
        osm.relation
            .iter()
            .fold(HashMap::<u64, Type>::new(), |mut acc, relation| {
                acc.insert(relation.id, check_relation_type(relation));
                acc
            });
    let way_to_type = osm
        .way
        .iter()
        .fold(HashMap::<u64, Type>::new(), |mut acc, way| {
            acc.insert(way.id, check_way_type(way));
            acc
        });

    let app = Router::new()
        .nest_service("/", ServeDir::new("../solid-leaflet-reprex/dist"))
        .route("/map/:z/:x/:y", get(render_tile_cache))
        .layer(Extension(Arc::new(Mutex::new(TileCache::new_no_default(
            filtered_osm.clone(),
            Arc::new(nodes_to_tile(filtered_osm.as_ref())),
            Arc::new(relation_to_type),
            Arc::new(way_to_type),
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
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::{build_index_for_zoom, load_binary_osm, render_tile_inner};

    #[tokio::test]
    async fn render_tile_test() {
        let osm = Arc::new(load_binary_osm());
        let index = build_index_for_zoom(osm.clone(), 13);
        let data = render_tile_inner(13, 4753, 2881, osm, &index).await;

        tokio::fs::write(&PathBuf::from("test-tile.png"), &data)
            .await
            .expect("storing rendition file");
    }
}
