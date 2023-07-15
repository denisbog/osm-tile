use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    sync::Arc,
};

use cairo::Context;
use log::debug;

use crate::{LoopWithType, Osm, Relation, Tag, Type, Way, TILE_SIZE};

pub fn convert_to_tile(lat: f64, lon: f64) -> (f64, f64) {
    let (lat_rad, lon_rad) = (lat.to_radians(), lon.to_radians());
    let x = lon_rad + PI;
    let y = PI - ((PI / 4f64) + (lat_rad / 2f64)).tan().ln();

    let rescale = |x: f64| x / (2f64 * PI);
    (rescale(x), rescale(y))
}
pub fn convert_to_int_tile(lat: f64, lon: f64) -> (i32, i32) {
    let tile_x = (lat / TILE_SIZE as f64) as i32;
    let tile_y = (lon / TILE_SIZE as f64) as i32;
    (tile_x, tile_y)
}
pub fn filter_relations(
    osm: &Osm,
    filter: &HashMap<String, HashSet<String>>,
) -> Vec<Arc<Relation>> {
    osm.relation
        .iter()
        .cloned()
        .filter(|relation| {
            if let Some(tag) = &relation.tag {
                tag.iter()
                    .filter(|item| {
                        filter.get(&item.k).is_some()
                            && filter.get(&item.k).unwrap().contains(&item.v)
                    })
                    .count()
                    .eq(&filter.len())
            } else {
                false
            }
        })
        .collect()
}

pub fn filter_ways_from_relations(osm: &Osm, relations: &[Arc<Relation>]) -> Vec<Arc<Way>> {
    let ways_to_filter: HashSet<u64> =
        relations
            .iter()
            .fold(HashSet::<u64>::new(), |mut acc, relation| {
                relation
                    .member
                    .iter()
                    .filter(|member| member.member_type.eq("way"))
                    .for_each(|way| {
                        acc.insert(way.member_ref);
                    });
                acc
            });

    osm.way
        .iter()
        .cloned()
        .filter(|way| ways_to_filter.contains(&way.id))
        .collect()
}

pub fn create_filter_expression() -> HashMap<String, HashSet<String>> {
    let mut filters = HashMap::<String, HashSet<String>>::new();
    filters.insert(
        "leisure".to_string(),
        HashSet::from_iter(
            vec!["park"]
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<String>>(),
        ),
    );
    filters
}

pub fn extract_loops_to_render(
    relation: &Relation,
    id_to_ways: &HashMap<u64, Arc<Way>>,
) -> Vec<LoopWithType> {
    let ways: Vec<&Arc<Way>> = relation
        .member
        .iter()
        .flat_map(|member| id_to_ways.get(&member.member_ref))
        .collect();

    let mut ways_to_visit = ways.iter().fold(HashSet::<u64>::new(), |mut acc, way| {
        acc.insert(way.id);
        acc
    });

    let segments = ways
        .iter()
        .fold(HashMap::<u64, HashSet<u64>>::new(), |mut acc, way| {
            acc.entry(way.nd.first().unwrap().reference)
                .or_insert(HashSet::<u64>::new())
                .insert(way.id);

            acc.entry(way.nd.last().unwrap().reference)
                .or_insert(HashSet::<u64>::new())
                .insert(way.id);
            acc
        });

    segments.iter().for_each(|(key, value)| {
        debug!("segments for {} {:?}", key, value);
    });

    let mut loops = Vec::<LoopWithType>::new();

    let a = ways.first().unwrap();
    loops.push(LoopWithType::new_with_type(a.id, check_way_type(a)));

    loops
        .last_mut()
        .unwrap()
        .memeber_loop
        .extend(a.nd.iter().map(|nd| nd.reference));
    ways_to_visit.remove(&a.id);

    while !ways_to_visit.is_empty() {
        debug!(
            "try to find next segment for {}",
            loops.last().unwrap().memeber_loop.last().unwrap()
        );
        let next_way = if let Some(next_way_to_process) = segments
            .get(loops.last().unwrap().memeber_loop.last().unwrap())
            .unwrap()
            .iter()
            .filter(|item| ways_to_visit.contains(item))
            .last()
        {
            let b = id_to_ways.get(next_way_to_process).unwrap();

            // check if we need to invert the sgment in order to mach the start of the new
            // segment with the end of the previous
            if loops.last().unwrap().memeber_loop.last().unwrap().eq(&b
                .nd
                .first()
                .unwrap()
                .reference)
            {
                loops
                    .last_mut()
                    .unwrap()
                    .memeber_loop
                    .extend(b.nd.iter().map(|nd| nd.reference));
            } else {
                loops
                    .last_mut()
                    .unwrap()
                    .memeber_loop
                    .extend(b.nd.iter().rev().map(|nd| nd.reference));
            };
            next_way_to_process
        } else {
            debug!("processing new loop");
            debug!("pick any out of {}", ways_to_visit.len());
            let pick_new_way = ways_to_visit.iter().next().unwrap();
            let a = id_to_ways.get(pick_new_way).unwrap();

            loops.push(LoopWithType::new_with_type(a.id, check_way_type(a)));

            loops
                .last_mut()
                .unwrap()
                .memeber_loop
                .extend(a.nd.iter().map(|nd| nd.reference));
            &a.id
        };

        ways_to_visit.remove(next_way);
    }

    loops.iter().for_each(|ordered_nodes| {
        debug!("nodes in order to render");
        ordered_nodes.memeber_loop.iter().for_each(|node| {
            debug!("node {}", node);
        });
    });
    loops
}

pub fn check_relation_type(relation: &Relation) -> Type {
    if let Some(tag) = &relation.tag {
        return check_tag_type(tag);
    }

    Type::Generic
}
pub fn check_way_type(way: &Way) -> Type {
    if let Some(tag) = &way.tag {
        return check_tag_type(tag);
    }
    Type::Generic
}

fn check_tag_type(tag: &[Tag]) -> Type {
    if tag.iter().any(|t| t.k.eq("leisure") && t.v.eq("park")) {
        return Type::Park;
    } else if tag.iter().any(|t| t.k.eq("landuse") && t.v.eq("forest")) {
        return Type::Forest;
    } else if tag.iter().any(|t| t.k.eq("building")) {
        return Type::Building;
    } else if tag.iter().any(|t| t.k.eq("natural") && t.v.eq("water")) {
        return Type::Water;
    } else if tag.iter().any(|t| t.k.eq("waterway")) {
        return Type::WaterRiver;
    };
    Type::Generic
}

pub fn set_context_for_type(way_type: &Type, context: &Context) {
    context.set_line_width(1f64);
    match *way_type {
        Type::Water | Type::WaterRiver => {
            context.set_source_rgb(0.286, 0.402, 0.510);
            context.set_line_width(3f64);
        }
        Type::Park => {
            context.set_source_rgb(0.442, 0.640, 0.551);
        }
        Type::Forest => {
            context.set_source_rgb(0.269, 0.480, 0.385);
        }
        Type::Building => {
            context.set_line_width(1f64);
            context.set_source_rgba(0.5, 0.5, 0.5, 0.2);
        }
        Type::Generic => {
            context.set_source_rgb(0.5, 0.5, 0.5);
        }
    }
}

pub fn end_context_for_way_type(way_type: &Type, context: &Context) {
    end_context_for_type(way_type, context, false);
}

pub fn end_context_for_relation_type(relation_type: &Type, context: &Context) {
    end_context_for_type(relation_type, context, true);
}

fn end_context_for_type(way_type: &Type, context: &Context, is_relation: bool) {
    match *way_type {
        Type::Water => {
            context.fill().unwrap();
        }
        Type::WaterRiver => {}
        Type::Park => {
            context.fill().unwrap();
        }
        Type::Forest => {
            context.fill().unwrap();
        }
        Type::Building => {
            if !is_relation {
                context.fill().unwrap();
            }
        }
        Type::Generic => {}
    }
}
