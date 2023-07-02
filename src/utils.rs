use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    sync::Arc,
};

use log::debug;

use crate::{Osm, Relation, Way, TILE_SIZE};

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
) -> Vec<Vec<u64>> {
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

    let mut loops = Vec::<Vec<u64>>::new();

    loops.push(Vec::<u64>::new());

    let a = ways.first().unwrap();

    loops
        .last_mut()
        .unwrap()
        .extend(a.nd.iter().map(|nd| nd.reference));
    ways_to_visit.remove(&a.id);

    while ways_to_visit.len() > 0 {
        debug!(
            "try to find next sgment for {}",
            loops.last().unwrap().last().unwrap()
        );
        let next_way = if let Some(next_way_to_process) = segments
            .get(loops.last().unwrap().last().unwrap())
            .unwrap()
            .iter()
            .filter(|item| ways_to_visit.contains(item))
            .last()
        {
            let b = id_to_ways.get(next_way_to_process).unwrap();

            // check if we need to invert the sgment in order to mach the start of the new
            // segment with the end of the previous
            if loops
                .last()
                .unwrap()
                .last()
                .unwrap()
                .eq(&b.nd.first().unwrap().reference)
            {
                loops
                    .last_mut()
                    .unwrap()
                    .extend(b.nd.iter().map(|nd| nd.reference));
            } else {
                loops
                    .last_mut()
                    .unwrap()
                    .extend(b.nd.iter().rev().map(|nd| nd.reference));
            };
            next_way_to_process
        } else {
            loops.push(Vec::<u64>::new());
            debug!("processing new loop");
            debug!("pick any out of {}", ways_to_visit.len());
            let pick_new_way = ways_to_visit.iter().next().unwrap();
            let a = id_to_ways.get(pick_new_way).unwrap();

            loops
                .last_mut()
                .unwrap()
                .extend(a.nd.iter().map(|nd| nd.reference));
            &a.id
        };

        ways_to_visit.remove(next_way);
    }

    loops.iter().for_each(|ordered_nodes| {
        debug!("nodes in order to render");
        ordered_nodes.iter().for_each(|node| {
            debug!("node {}", node);
        });
    });
    loops
}
