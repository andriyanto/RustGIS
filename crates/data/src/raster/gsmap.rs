//! GSMaP rain grids (JAXA's Global Satellite Mapping of Precipitation) in their plain binary
//! form: GSMaP_NOW and GSMaP_Gauge_NOW (`gsmap_now.YYYYMMDD.HHNN.dat.gz`,
//! `gsmap_gauge_now.…`), and the other products of the same layout (NRT, MVK, Gauge).
//!
//! As JAXA's *Data Format Description for GSMaP_NOW* gives it: 4-byte floats, little-endian,
//! gzip-compressed; 3600 × 1200 cells of 0.1° from 60° N to 60° S, the first cell's centre at
//! 0.05° E, 59.95° N, rows north to south and longitude east from 0°; rain rate in mm/h, a
//! negative value missing (−4 sea ice, −8 low temperature, −99 no observation). The daily 0.25°
//! grids (1440 × 480, first centre 0.125° E, 59.875° N) are read too.
//!
//! The grid is turned to start at 180° W, as RustGIS's maps are, and missing cells are NaN (the
//! reason for one is not kept). The file is hostile: its size must be one of the grids', and the
//! gzip stream is checked (header, CRC-32, length) and inflated to at most the grid's size.

use super::{ColorHint, Crs, GeoTransform, Raster};
use crate::{DataError, Result};

/// The two grids: cells across and down, and cell size (degrees). Both span 60° N to 60° S from 0° E.
const GRIDS: [(u32, u32, f64); 2] = [(3600, 1200, 0.1), (1440, 480, 0.25)];

/// The largest compressed file read (a grid compresses to a few MB; 64 MB is far beyond).
pub const MAX_FILE_BYTES: usize = 64 << 20;

/// Whether a file name is a GSMaP binary grid's: `gsmap…` ending `.dat` or `.dat.gz`.
pub fn is_gsmap_name(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    base.starts_with("gsmap") && (base.ends_with(".dat") || base.ends_with(".dat.gz"))
}

/// Reads a GSMaP grid from a file's name and bytes (gzip-compressed or not).
pub fn read(file_name: &str, bytes: &[u8]) -> Result<Raster> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(bad("the file is larger than a GSMaP grid"));
    }
    let largest = GRIDS.iter().map(|(w, h, _)| (*w as usize) * (*h as usize) * 4).max().unwrap_or(0);
    let raw = if bytes.starts_with(&[0x1f, 0x8b]) { gunzip(bytes, largest)? } else { bytes.to_vec() };
    let (w, h, cell) = GRIDS
        .iter()
        .copied()
        .find(|(w, h, _)| (*w as usize) * (*h as usize) * 4 == raw.len())
        .ok_or_else(|| bad(format!("{} bytes is not a GSMaP grid (3600 × 1200 or 1440 × 480 floats)", raw.len())))?;
    let (wu, hu) = (w as usize, h as usize);
    // From 0° E eastward, turned to start at 180° W: column k of the map is column
    // (k + half) of the file.
    let half = wu / 2;
    let mut data = vec![f32::NAN; wu * hu];
    for row in 0..hu {
        for (k, out) in data[row * wu..(row + 1) * wu].iter_mut().enumerate() {
            let i = (row * wu + (k + half) % wu) * 4;
            let v = f32::from_le_bytes([raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]);
            *out = if v.is_finite() && v >= 0.0 { v } else { f32::NAN };
        }
    }
    let transform = GeoTransform { origin_x: -180.0, origin_y: 60.0, pixel_width: cell, pixel_height: -cell };
    let mut r = Raster::new(label(file_name), w, h, vec![data], transform, Crs::LonLat, None, ColorHint::Gray, "Float32")?;
    if let Some(b) = r.bands.first_mut() {
        b.name = "Rain rate (mm/h)".into();
    }
    Ok(r)
}

/// A layer name from the file's: the product and its start time, as `GSMaP_NOW 2025-11-27 06:00
/// UTC`; the file's own name when it doesn't follow JAXA's naming.
fn label(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = base.trim_end_matches(".gz").trim_end_matches(".GZ").trim_end_matches(".dat").trim_end_matches(".DAT");
    let parts: Vec<&str> = stem.split('.').collect();
    let product = match parts.first().map(|p| p.to_ascii_lowercase()) {
        Some(p) if p == "gsmap_now" => "GSMaP_NOW".to_string(),
        Some(p) if p == "gsmap_gauge_now" => "GSMaP_Gauge_NOW".to_string(),
        Some(p) if p == "gsmap_nrt" => "GSMaP_NRT".to_string(),
        Some(p) if p == "gsmap_gauge" => "GSMaP_Gauge".to_string(),
        Some(p) if p == "gsmap_mvk" => "GSMaP_MVK".to_string(),
        _ => return stem.to_string(),
    };
    let date = parts.get(1).filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()));
    let time = parts.get(2).map(|t| t.split('_').next().unwrap_or(t)).filter(|t| t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()));
    match (date, time) {
        (Some(d), Some(t)) => format!("{product} {}-{}-{} {}:{} UTC", &d[..4], &d[4..6], &d[6..], &t[..2], &t[2..]),
        (Some(d), None) => format!("{product} {}-{}-{}", &d[..4], &d[4..6], &d[6..]),
        _ => stem.to_string(),
    }
}

fn bad(msg: impl std::fmt::Display) -> DataError {
    DataError::Format(format!("GSMaP: {msg}"))
}

/// A gzip member's contents (RFC 1952), at most `limit` bytes, its CRC-32 and length checked.
fn gunzip(bytes: &[u8], limit: usize) -> Result<Vec<u8>> {
    let need = |n: usize| bytes.get(..n).ok_or_else(|| bad("the gzip header is cut short"));
    let head = need(10)?;
    if head[2] != 8 {
        return Err(bad("the gzip stream isn't deflate"));
    }
    let flags = head[3];
    let mut pos = 10usize;
    if flags & 0x04 != 0 {
        let x = need(pos + 2)?;
        pos += 2 + usize::from(u16::from_le_bytes([x[pos], x[pos + 1]]));
    }
    for bit in [0x08, 0x10] {
        if flags & bit != 0 {
            let end = bytes.get(pos..).and_then(|b| b.iter().position(|&c| c == 0)).ok_or_else(|| bad("the gzip header is cut short"))?;
            pos += end + 1;
        }
    }
    if flags & 0x02 != 0 {
        pos += 2;
    }
    let body = bytes.get(pos..bytes.len().saturating_sub(8)).ok_or_else(|| bad("the gzip stream is cut short"))?;
    let out =
        miniz_oxide::inflate::decompress_to_vec_with_limit(body, limit).map_err(|_| bad("the gzip stream is damaged or too large"))?;
    let tail = &bytes[bytes.len() - 8..];
    let crc = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    let len = u32::from_le_bytes([tail[4], tail[5], tail[6], tail[7]]);
    if crc32(&out) != crc || len != out.len() as u32 {
        return Err(bad("the gzip stream's checksum doesn't match: the file is damaged"));
    }
    Ok(out)
}

/// CRC-32 (IEEE), as gzip checks it.
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rustgis_geom::Coord;

    /// A gzip member of `data` (stored with deflate), as JAXA's files are.
    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
        out.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
        out.extend(crc32(data).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out
    }

    /// A 0.1° grid as the file holds it: everything 0, a few cells set, by (lon 0–360, lat).
    fn grid(cells: &[(f64, f64, f32)]) -> Vec<u8> {
        let mut v = vec![0f32; 3600 * 1200];
        for &(lon, lat, x) in cells {
            let col = ((lon - 0.05) / 0.1).round() as usize;
            let row = ((59.95 - lat) / 0.1).round() as usize;
            v[row * 3600 + col] = x;
        }
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn reads_rain_where_it_fell() {
        // Padang (100.35° E, 0.95° S), the first cell, a western cell, and missing ones.
        let bytes = gzip(&grid(&[
            (100.35, -0.95, 29.5),
            (0.05, 59.95, 1.25),
            (359.95, -59.95, 3.0),
            (100.45, -0.95, -99.0),
            (100.55, -0.95, -4.0),
        ]));
        let r = read("gsmap_now.20251127.0600.dat.gz", &bytes).unwrap();
        assert_eq!(r.name, "GSMaP_NOW 2025-11-27 06:00 UTC");
        assert_eq!((r.width, r.height), (3600, 1200));
        assert_eq!(r.bands[0].name, "Rain rate (mm/h)");
        let at = |lon: f64, lat: f64| {
            let (c, row) = r.pixel_at(Coord::new(lon, lat)).unwrap();
            r.value(0, c, row)
        };
        assert_eq!(at(100.35, -0.95), Some(29.5));
        assert_eq!(at(0.05, 59.95), Some(1.25));
        // 359.95° E is 0.05° W.
        assert_eq!(at(-0.05, -59.95), Some(3.0));
        assert_eq!((at(100.45, -0.95), at(100.55, -0.95)), (None, None), "missing cells have no value");
        assert_eq!(at(110.05, 5.05), Some(0.0));
        let e = r.extent();
        assert_eq!((e.min_x, e.max_x, e.min_y, e.max_y), (-180.0, 180.0, -60.0, 60.0));
    }

    #[test]
    fn reads_an_uncompressed_grid_and_names_the_gauge_product() {
        let r = read("gsmap_gauge_now.20251127.0630.dat", &grid(&[(100.35, -0.95, 49.7)])).unwrap();
        assert_eq!(r.name, "GSMaP_Gauge_NOW 2025-11-27 06:30 UTC");
        let (c, row) = r.pixel_at(Coord::new(100.35, -0.95)).unwrap();
        assert_eq!(r.value(0, c, row), Some(49.7));
    }

    #[test]
    fn reads_the_daily_quarter_degree_grid() {
        let mut v = vec![0f32; 1440 * 480];
        v[0] = 7.5;
        let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let r = read("gsmap_gauge.20251127.0.25d.daily.00Z-23Z.dat.gz", &gzip(&bytes)).unwrap();
        assert_eq!((r.width, r.height), (1440, 480));
        let (c, row) = r.pixel_at(Coord::new(0.125, 59.875)).unwrap();
        assert_eq!(r.value(0, c, row), Some(7.5));
    }

    #[test]
    fn refuses_what_is_not_a_grid() {
        assert!(read("gsmap_now.x.dat", &[0u8; 1000]).is_err());
        let mut damaged = gzip(&grid(&[]));
        let n = damaged.len();
        damaged[n - 8] ^= 1;
        assert!(read("gsmap_now.x.dat.gz", &damaged).unwrap_err().to_string().contains("checksum"));
        assert!(read("gsmap_now.x.dat.gz", &[0x1f, 0x8b, 8]).is_err());
        // Inflating stops at the grid's size.
        let big = gzip(&vec![0u8; 3600 * 1200 * 4 + 4]);
        assert!(read("gsmap_now.x.dat.gz", &big).is_err());
    }

    #[test]
    fn names() {
        assert!(is_gsmap_name("/now/half_hour/2025/11/27/gsmap_now.20251127.0600.dat.gz"));
        assert!(is_gsmap_name("GSMAP_GAUGE_NOW.20251127.0600.DAT"));
        assert!(!is_gsmap_name("gsmap_now_rain.20251127.0600.nc"));
        assert!(!is_gsmap_name("rain.dat.gz"));
        assert_eq!(label("gsmap_now.20251127.0600_0700.dat.gz"), "GSMaP_NOW 2025-11-27 06:00 UTC");
        assert_eq!(label("gsmap_x.dat"), "gsmap_x");
    }
}
