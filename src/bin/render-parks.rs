use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
    sync::Arc,
};

use cairo::{Context, ImageSurface};
use env_logger::Env;
use log::info;
use osm_tiles::{
    utils::{convert_to_tile, extract_loops_to_render, set_context_for_type},
    NodeToTile, Osm, Type, Way, TILE_SIZE,
};

const PADDING: f64 = 100f64;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let zoom = 16;
    let osm: Osm =
        quick_xml::de::from_reader(BufReader::new(File::open("temp.xml").unwrap())).unwrap();

    let nodes_to_tile: NodeToTile =
        osm.node
            .iter()
            .fold(HashMap::<u64, (f64, f64)>::new(), |mut acc, item| {
                acc.insert(item.id, convert_to_tile(item.lat, item.lon));
                acc
            });

    let dimension_in_pixels_for_zoom = f64::from(TILE_SIZE * (1 << zoom));

    let mapped_nodes: NodeToTile = nodes_to_tile
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

    let id_to_ways = osm
        .way
        .iter()
        .fold(HashMap::<u64, Arc<Way>>::new(), |mut acc, way| {
            acc.insert(way.id, way.clone());
            acc
        });

    let filtered_nodes: Vec<&(f64, f64)> = osm
        .relation
        .iter()
        .flat_map(|relation| relation.member.iter())
        .map(|relation| relation.member_ref)
        .flat_map(|member| id_to_ways.get(&member))
        .flat_map(|way| way.nd.iter())
        .map(|nd| nd.reference)
        .flat_map(|node| mapped_nodes.get(&node))
        .collect();

    let mut min_x = filtered_nodes
        .iter()
        .map(|(x, _y)| *x)
        .reduce(f64::min)
        .unwrap();
    let mut min_y = filtered_nodes
        .iter()
        .map(|(_x, y)| *y)
        .reduce(f64::min)
        .unwrap();

    let mut max_x = filtered_nodes
        .iter()
        .map(|(x, _y)| *x)
        .reduce(f64::max)
        .unwrap();
    let mut max_y = filtered_nodes
        .iter()
        .map(|(_x, y)| *y)
        .reduce(f64::max)
        .unwrap();

    min_x -= PADDING;
    min_y -= PADDING;
    max_x += PADDING;
    max_y += PADDING;

    info!("min_x {} min_y {}", min_x, min_y);

    let width = max_x - min_x;
    let height = max_y - min_y;
    let surface = ImageSurface::create(cairo::Format::Rgb24, width as i32, height as i32).unwrap();
    let context = Context::new(&surface).unwrap();
    context.set_source_rgb(0.2, 0.2, 0.2);
    context.paint().unwrap();

    context.set_line_width(1f64);
    context.set_line_cap(cairo::LineCap::Round);
    context.set_line_join(cairo::LineJoin::Round);

    context.set_source_rgb(0.5, 0.5, 0.5);
    context.set_line_width(1f64);

    info!("init nodes to order");
    osm.relation.iter().for_each(|relation| {
        relation
            .member
            .iter()
            .map(|relation| relation.member_ref)
            .flat_map(|relation| id_to_ways.get(&relation))
            .for_each(|way| {
                way.nd
                    .iter()
                    .for_each(|node| info!("node {}-{}", way.id, node.reference));
            })
    });

    context.set_source_rgba(0.5, 1.0, 0.5, 0.2);

    osm.relation.iter().for_each(|relation| {
        let loops = extract_loops_to_render(relation.as_ref(), &id_to_ways);

        loops.iter().for_each(|ordered_nodes| {
            let way_type = &ordered_nodes.member_type;
            set_context_for_type(way_type, &context);

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

            if let Type::Park | Type::Building = way_type {
                context.fill().unwrap();
            }
            context.stroke().unwrap();
        });

        context.stroke().unwrap();
    });

    context.set_source_rgb(0.7, 0.7, 0.7);
    context.line_to(width, 0 as f64);
    context.line_to(0 as f64, 0 as f64);
    context.line_to(0 as f64, height);
    context.stroke().unwrap();

    let mut buffer = BufWriter::new(Vec::<u8>::new());
    surface.write_to_png(&mut buffer).unwrap();
    let new_path = PathBuf::from("render-park.png");
    tokio::fs::write(&new_path, &buffer.into_inner().unwrap())
        .await
        .expect("storing rendition file");
}
