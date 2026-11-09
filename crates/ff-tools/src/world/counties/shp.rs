//! Just enough of the ESRI shapefile format to read the Census Bureau's
//! county boundaries: polygon records from the `.shp`, text columns from
//! the `.dbf` beside it. Both files are little-endian apart from the `.shp`
//! record headers; the layout is the 1998 ESRI technical description.

use std::path::Path;

use crate::util::Result;

/// One polygon: its rings as `(lon, lat)` vertices and its bounding box
/// `(min_lon, min_lat, max_lon, max_lat)`.
#[derive(Debug, Clone, Default)]
pub struct Polygon {
    pub rings: Vec<Vec<(f64, f64)>>,
    pub bbox: (f64, f64, f64, f64),
}

impl Polygon {
    /// Even-odd ray cast over every ring, so holes come out right without
    /// knowing which ring is which.
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        let (x0, y0, x1, y1) = self.bbox;
        if lon < x0 || lon > x1 || lat < y0 || lat > y1 {
            return false;
        }
        let mut inside = false;
        for ring in &self.rings {
            let n = ring.len();
            if n < 3 {
                continue;
            }
            let mut j = n - 1;
            for i in 0..n {
                let (xi, yi) = ring[i];
                let (xj, yj) = ring[j];
                if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
        }
        inside
    }
}

fn le_i32(bytes: &[u8], at: usize) -> Result<i32> {
    bytes
        .get(at..at + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("shapefile ends early at byte {at}"))
}

fn be_i32(bytes: &[u8], at: usize) -> Result<i32> {
    bytes
        .get(at..at + 4)
        .map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("shapefile ends early at byte {at}"))
}

fn le_f64(bytes: &[u8], at: usize) -> Result<f64> {
    let b = bytes
        .get(at..at + 8)
        .ok_or_else(|| format!("shapefile ends early at byte {at}"))?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Ok(f64::from_le_bytes(a))
}

fn index(value: i32, what: &str) -> Result<usize> {
    usize::try_from(value).map_err(|_| format!("negative {what} in the shapefile"))
}

/// Every record of a polygon `.shp`, in file order. A null shape reads as an
/// empty polygon so the record numbers stay aligned with the `.dbf`.
pub fn read_polygons(path: &Path) -> Result<Vec<Polygon>> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if be_i32(&bytes, 0)? != 9994 {
        return Err(format!("{} is not a shapefile", path.display()));
    }
    let mut out = Vec::new();
    let mut at = 100;
    while at + 8 <= bytes.len() {
        let words = index(be_i32(&bytes, at + 4)?, "record length")?;
        let body = at + 8;
        let end = body + words * 2;
        at = end;
        let kind = le_i32(&bytes, body)?;
        if kind == 0 {
            out.push(Polygon::default());
            continue;
        }
        if kind != 5 {
            return Err(format!(
                "{}: shape type {kind}, expected polygons (5)",
                path.display()
            ));
        }
        let bbox = (
            le_f64(&bytes, body + 4)?,
            le_f64(&bytes, body + 12)?,
            le_f64(&bytes, body + 20)?,
            le_f64(&bytes, body + 28)?,
        );
        let parts = index(le_i32(&bytes, body + 36)?, "part count")?;
        let points = index(le_i32(&bytes, body + 40)?, "point count")?;
        let mut starts = Vec::with_capacity(parts + 1);
        for p in 0..parts {
            starts.push(index(le_i32(&bytes, body + 44 + p * 4)?, "part start")?);
        }
        starts.push(points);
        let first_point = body + 44 + parts * 4;
        let mut rings = Vec::with_capacity(parts);
        for pair in starts.windows(2) {
            let mut ring = Vec::with_capacity(pair[1].saturating_sub(pair[0]));
            for k in pair[0]..pair[1] {
                let off = first_point + k * 16;
                ring.push((le_f64(&bytes, off)?, le_f64(&bytes, off + 8)?));
            }
            rings.push(ring);
        }
        out.push(Polygon { rings, bbox });
    }
    Ok(out)
}

/// Every record of a `.dbf` as `(column name, trimmed text)` pairs. The
/// Census files are UTF-8 (their `.cpg` says so); deleted records are kept
/// as empty rows so the record numbers stay aligned with the `.shp`.
pub fn read_table(path: &Path) -> Result<Vec<Vec<(String, String)>>> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let count = index(le_i32(&bytes, 4)?, "record count")?;
    let header = bytes
        .get(8..12)
        .map(|b| {
            (
                usize::from(u16::from_le_bytes([b[0], b[1]])),
                usize::from(u16::from_le_bytes([b[2], b[3]])),
            )
        })
        .ok_or("dbf header ends early")?;
    let (header_len, record_len) = header;
    let mut fields: Vec<(String, usize)> = Vec::new();
    let mut at = 32;
    while at + 32 <= header_len && bytes.get(at) != Some(&0x0D) {
        let name: String = bytes[at..at + 11]
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| char::from(*b))
            .collect();
        fields.push((name, usize::from(bytes[at + 16])));
        at += 32;
    }
    let mut rows = Vec::with_capacity(count);
    for r in 0..count {
        let start = header_len + r * record_len;
        let record = bytes
            .get(start..start + record_len)
            .ok_or_else(|| format!("{}: record {r} ends early", path.display()))?;
        if record[0] == b'*' {
            rows.push(Vec::new());
            continue;
        }
        let mut off = 1;
        let mut row = Vec::with_capacity(fields.len());
        for (name, width) in &fields {
            let text = String::from_utf8_lossy(&record[off..off + width])
                .trim()
                .to_string();
            row.push((name.clone(), text));
            off += width;
        }
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hole_is_outside_its_polygon() {
        let square = |a: f64, b: f64| vec![(a, a), (b, a), (b, b), (a, b), (a, a)];
        let polygon = Polygon {
            rings: vec![square(0.0, 10.0), square(4.0, 6.0)],
            bbox: (0.0, 0.0, 10.0, 10.0),
        };
        assert!(polygon.contains(2.0, 2.0));
        assert!(!polygon.contains(5.0, 5.0));
        assert!(!polygon.contains(11.0, 5.0));
    }
}
