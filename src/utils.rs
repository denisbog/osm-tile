use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    sync::Arc,
};

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
