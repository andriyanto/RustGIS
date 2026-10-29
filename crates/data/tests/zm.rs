//! Heights and measures (Z, M) beside the 2D geometry: read from and written to Shapefiles,
//! GeoJSON and WKB, kept with the vertex they belong to, and dropped when the vertices change.

use rustgis_data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value, VertexZm, io, wkb};

fn c(x: f64, y: f64) -> Coord {
    Coord::new(x, y)
}

/// A survey-like layer: two cross-sections, the second of two parts.
fn sections() -> Dataset {
    let mut ds = Dataset::new("sections", GeometryType::Polyline, vec![Field::new("name", FieldType::Text)]);
    ds.push_next_zm(
        Some(Geometry::Line(vec![vec![c(100.0, -1.0), c(100.001, -1.0), c(100.002, -1.0)]])),
        vec![Value::Text("BM1".into())],
        Some(VertexZm { z: Some(vec![12.5, 9.25, 12.75]), m: Some(vec![0.0, 50.0, 100.0]) }),
    );
    ds.push_next_zm(
        Some(Geometry::Line(vec![vec![c(100.0, -1.1), c(100.001, -1.1)], vec![c(100.002, -1.1), c(100.003, -1.1)]])),
        vec![Value::Text("BM2".into())],
        Some(VertexZm { z: Some(vec![11.0, 8.5, 8.75, 11.25]), m: Some(vec![0.0, 40.0, 80.0, 120.0]) }),
    );
    ds
}

fn shapefile_round_trip(ds: &Dataset) -> Dataset {
    let (parts, left_out) = io::shapefile_parts(ds).unwrap();
    assert_eq!(left_out, 0);
    let files: Vec<(String, Vec<u8>)> = parts.into_iter().map(|(ext, b)| (format!("{}.{ext}", ds.name), b)).collect();
    let mut layers = io::read_zip("layer.zip", &rustgis_data::zip::write(&files).unwrap()).unwrap();
    layers.remove(0).0
}

fn shp_type(ds: &Dataset) -> i32 {
    let (parts, _) = io::shapefile_parts(ds).unwrap();
    let shp = &parts.iter().find(|(e, _)| *e == "shp").unwrap().1;
    i32::from_le_bytes(shp[32..36].try_into().unwrap())
}

#[test]
fn polylines_keep_their_heights_and_measures_through_a_shapefile() {
    let ds = sections();
    assert!(ds.has_z() && ds.has_m());
    assert_eq!(shp_type(&ds), 13, "PolylineZ");
    let back = shapefile_round_trip(&ds);
    assert_eq!(back.features.len(), 2);
    for (a, b) in ds.features.iter().zip(back.features.iter()) {
        assert_eq!(a.geometry, b.geometry);
        assert_eq!(ds.zm_of(a), back.zm_of(b), "feature {}", a.id);
    }
}

#[test]
fn points_and_polygons_keep_their_heights() {
    let mut pts = Dataset::new("bm", GeometryType::Point, vec![]);
    pts.push_next_zm(Some(Geometry::Point(c(100.0, -1.0))), vec![], Some(VertexZm { z: Some(vec![25.125]), m: None }));
    pts.push_next_zm(Some(Geometry::Point(c(100.1, -1.0))), vec![], None);
    assert_eq!(shp_type(&pts), 11, "PointZ");
    let back = shapefile_round_trip(&pts);
    assert_eq!(back.zm_of(&back.features[0]).and_then(|z| z.z.clone()), Some(vec![25.125]));
    // A point without a height is written at 0, as GDAL does.
    assert_eq!(back.zm_of(&back.features[1]).and_then(|z| z.z.clone()), Some(vec![0.0]));

    // A pond with an island, its rings closed as a Shapefile writes them.
    let outer = vec![c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0), c(1.0, 0.0), c(0.0, 0.0)];
    let hole = vec![c(0.4, 0.4), c(0.6, 0.4), c(0.6, 0.6), c(0.4, 0.6), c(0.4, 0.4)];
    let mut poly = Dataset::new("pond", GeometryType::Polygon, vec![]);
    // Each ring closed at its first vertex's height.
    let z = vec![5.0, 6.0, 7.0, 8.0, 5.0, 10.0, 11.0, 12.0, 13.0, 10.0];
    poly.push_next_zm(Some(Geometry::Polygon(vec![vec![outer, hole]])), vec![], Some(VertexZm { z: Some(z), m: None }));
    assert_eq!(shp_type(&poly), 15, "PolygonZ");
    let back = shapefile_round_trip(&poly);
    let (f0, b0) = (&poly.features[0], &back.features[0]);
    assert_eq!(f0.geometry, b0.geometry);
    assert_eq!(poly.zm_of(f0).and_then(|z| z.z.clone()), back.zm_of(b0).and_then(|z| z.z.clone()));

    // A ring whose closing vertex has another height is closed with the first vertex's.
    let mut odd = poly.clone();
    if let Some(zm) = odd.zm.values_mut().next() {
        zm.z.as_mut().unwrap()[4] = 99.0;
    }
    let back = shapefile_round_trip(&odd);
    assert_eq!(back.features[0].geometry, odd.features[0].geometry, "no extra vertex");
    assert_eq!(back.zm_of(&back.features[0]).and_then(|z| z.z.clone()).map(|z| z[4]), Some(5.0));
}

#[test]
fn measures_alone_make_m_shapes() {
    let mut ds = sections();
    for zm in ds.zm.values_mut() {
        zm.z = None;
    }
    assert!(!ds.has_z() && ds.has_m());
    assert_eq!(shp_type(&ds), 23, "PolylineM");
    let back = shapefile_round_trip(&ds);
    assert_eq!(back.zm_of(&back.features[1]).and_then(|z| z.m.clone()), Some(vec![0.0, 40.0, 80.0, 120.0]));
    assert!(!back.has_z());
}

#[test]
fn a_layer_without_heights_stays_2d() {
    let mut ds = Dataset::new("plain", GeometryType::Point, vec![]);
    ds.push(Some(Geometry::Point(c(1.0, 2.0))), vec![]);
    assert_eq!(shp_type(&ds), 1);
    assert!(shapefile_round_trip(&ds).zm.is_empty());
    let gj = io::to_geojson(&ds);
    assert_eq!(gj["features"][0]["geometry"]["coordinates"], serde_json::json!([1.0, 2.0]));
}

#[test]
fn geojson_keeps_the_third_coordinate() {
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"n":1},"geometry":{"type":"LineString","coordinates":[[100,-1,12.5],[100.001,-1,9.25],[100.002,-1]]}},
        {"type":"Feature","properties":{"n":2},"geometry":{"type":"Point","coordinates":[100,-1]}}]}"#;
    let (ds, _) = io::read_geojson(text, "s").unwrap();
    let z = ds.zm_of(&ds.features[0]).and_then(|zm| zm.z.clone()).unwrap();
    assert_eq!(z[..2], [12.5, 9.25]);
    assert!(z[2].is_nan(), "a position with two numbers has no height");
    assert!(ds.zm_of(&ds.features[1]).is_none());
    let out = io::to_geojson(&ds);
    assert_eq!(out["features"][0]["geometry"]["coordinates"][0], serde_json::json!([100.0, -1.0, 12.5]));
    assert_eq!(out["features"][0]["geometry"]["coordinates"][2], serde_json::json!([100.002, -1.0]));
    let (again, _) = io::read_geojson(&out.to_string(), "s").unwrap();
    assert_eq!(again.zm_of(&again.features[0]).and_then(|zm| zm.z.clone()).map(|z| z[..2].to_vec()), Some(vec![12.5, 9.25]));
}

#[test]
fn wkb_reads_and_writes_z_m_and_zm() {
    let g = Geometry::Line(vec![vec![c(1.0, 2.0), c(3.0, 4.0)], vec![c(5.0, 6.0), c(7.0, 8.0)]]);
    for zm in [
        VertexZm { z: Some(vec![10.0, 11.0, 12.0, 13.0]), m: None },
        VertexZm { z: None, m: Some(vec![0.0, 1.0, 2.0, 3.0]) },
        VertexZm { z: Some(vec![10.0, 11.0, 12.0, 13.0]), m: Some(vec![0.0, 1.0, 2.0, 3.0]) },
    ] {
        let bytes = wkb::encode_zm(&g, Some(&zm));
        assert!(wkb::has_zm(&bytes));
        assert_eq!(wkb::decode(&bytes), Some(g.clone()), "plain decode drops Z and M");
        assert_eq!(wkb::decode_zm(&bytes), Some((g.clone(), Some(zm))));
    }
    assert_eq!(wkb::encode_zm(&g, None), wkb::encode(&g));
    assert_eq!(wkb::decode_zm(&wkb::encode(&g)), Some((g.clone(), None)));
    let poly = Geometry::Polygon(vec![vec![vec![c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0), c(0.0, 0.0)]]]);
    let zm = VertexZm { z: Some(vec![1.0, 2.0, 3.0, 1.0]), m: None };
    assert_eq!(wkb::decode_zm(&wkb::encode_zm(&poly, Some(&zm))), Some((poly, Some(zm))));
}

#[test]
fn wkb_keeps_heights_with_their_vertices_when_members_are_dropped() {
    // A MultiPoint Z whose middle point is empty (NaN x, y): the plain geometry drops it, and the
    // heights must drop with it.
    let mut b = vec![1u8];
    b.extend_from_slice(&1004u32.to_le_bytes());
    b.extend_from_slice(&3u32.to_le_bytes());
    for (x, y, z) in [(1.0, 1.0, 10.0), (f64::NAN, f64::NAN, 20.0), (2.0, 2.0, 30.0)] {
        b.push(1);
        b.extend_from_slice(&1001u32.to_le_bytes());
        for v in [x, y, z] {
            b.extend_from_slice(&f64::to_le_bytes(v));
        }
    }
    let (g, zm) = wkb::decode_zm(&b).unwrap();
    assert_eq!(g, Geometry::MultiPoint(vec![c(1.0, 1.0), c(2.0, 2.0)]));
    assert_eq!(zm.and_then(|z| z.z), Some(vec![10.0, 30.0]));
}

#[test]
fn heights_that_no_longer_fit_are_ignored() {
    let mut ds = sections();
    // A vertex added by an edit: the heights can't be placed any more.
    if let Some(Geometry::Line(parts)) = ds.features[0].geometry.as_mut() {
        parts[0].push(c(100.003, -1.0));
    }
    assert!(ds.zm_of(&ds.features[0]).is_none());
    assert!(ds.zm_of(&ds.features[1]).is_some());
    ds.prune_zm();
    assert_eq!(ds.zm.len(), 1);
    // A vertex moved: same count, the heights stay (a move is horizontal).
    if let Some(Geometry::Line(parts)) = ds.features[1].geometry.as_mut() {
        parts[0][0] = c(100.0005, -1.1);
    }
    assert!(ds.zm_of(&ds.features[1]).is_some());
}

#[test]
fn heights_survive_reprojection_and_a_project_save() {
    let ds = sections();
    let utm = rustgis_data::raster::Crs::Epsg(32748);
    let projected = io::project(&ds, utm).unwrap();
    assert_eq!(projected.zm_of(&projected.features[1]), ds.zm_of(&ds.features[1]));
    let json = serde_json::to_string(&ds).unwrap();
    let back: Dataset = serde_json::from_str(&json).unwrap();
    assert_eq!(back.zm, ds.zm);
    // A layer without heights saves as before (no `zm` key).
    let plain = Dataset::new("p", GeometryType::Point, vec![]);
    assert!(!serde_json::to_string(&plain).unwrap().contains("\"zm\""));
}
