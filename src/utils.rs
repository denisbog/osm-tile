use std::{
    collections::{HashMap, HashSet},
    f64::consts::PI,
    sync::Arc,
};

use crate::{Way, TILE_SIZE};

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
pub fn filter(way: Vec<Arc<Way>>, filter: &HashMap<String, HashSet<String>>) -> Vec<Arc<Way>> {
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
pub fn creat_filter() -> HashMap<String, HashSet<String>> {
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
    filters
}
