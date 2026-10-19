//! "Download annotated PDF": merges the saved sentence highlights and box
//! annotations into a copy of the paper. The original upload is never modified.
//!
//! Sentence marks become a translucent filled band; box marks become a thin
//! translucent fill with a solid coloured border — the same look as the original
//! ReportLab/pypdf overlay. Coordinates are stored normalised (0..1, origin at
//! the top-left of the *rendered* page), so page rotation is taken into account.

use std::collections::BTreeMap;

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use crate::models::Annotation;
use crate::util::is_hex_color;

const GS_FILL: &str = "RDFill";
const GS_BOX: &str = "RDBox";

/// Cheap sanity check used on upload. Returns the page count when the PDF
/// structure can be parsed.
pub fn probe(bytes: &[u8]) -> Result<usize, String> {
    let doc = Document::load_mem(bytes).map_err(|e| e.to_string())?;
    Ok(doc.get_pages().len())
}

fn hex_to_rgb(color: &str) -> (f32, f32, f32) {
    let value = if is_hex_color(color) { color } else { "#ffe66d" };
    let channel = |range: std::ops::Range<usize>| -> f32 {
        u8::from_str_radix(&value[range], 16).unwrap_or(255) as f32 / 255.0
    };
    (channel(1..3), channel(3..5), channel(5..7))
}

fn as_f32(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

/// Follow the page-tree `Parent` chain to find an inheritable attribute.
fn inherited(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut current = page_id;
    for _ in 0..64 {
        let dict = doc.get_dictionary(current).ok()?;
        if let Ok(value) = dict.get(key) {
            let resolved = match value {
                Object::Reference(id) => doc.get_object(*id).ok()?.clone(),
                other => other.clone(),
            };
            return Some(resolved);
        }
        current = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn media_box(doc: &Document, page_id: ObjectId) -> (f32, f32, f32, f32) {
    if let Some(Object::Array(items)) = inherited(doc, page_id, b"MediaBox") {
        let values: Vec<f32> = items
            .iter()
            .filter_map(|item| match item {
                Object::Reference(id) => doc.get_object(*id).ok().and_then(as_f32),
                other => as_f32(other),
            })
            .collect();
        if values.len() == 4 {
            let (x0, x1) = (values[0].min(values[2]), values[0].max(values[2]));
            let (y0, y1) = (values[1].min(values[3]), values[1].max(values[3]));
            if x1 - x0 > 0.0 && y1 - y0 > 0.0 {
                return (x0, y0, x1, y1);
            }
        }
    }
    (0.0, 0.0, 612.0, 792.0)
}

fn rotation(doc: &Document, page_id: ObjectId) -> i64 {
    match inherited(doc, page_id, b"Rotate") {
        Some(Object::Integer(r)) => r.rem_euclid(360),
        _ => 0,
    }
}

/// Convert a normalised rectangle (top-left origin, rendered orientation) into
/// unrotated PDF user space (bottom-left origin) relative to the media box.
fn map_rect(rot: i64, w: f32, h: f32, nx: f32, ny: f32, nw: f32, nh: f32) -> (f32, f32, f32, f32) {
    let (vw, vh) = if rot == 90 || rot == 270 { (h, w) } else { (w, h) };
    let (vx0, vy0) = (nx * vw, ny * vh);
    let (vx1, vy1) = ((nx + nw) * vw, (ny + nh) * vh);
    let map = |vx: f32, vy: f32| -> (f32, f32) {
        match rot {
            90 => (vy, vx),
            180 => (w - vx, vy),
            270 => (w - vy, h - vx),
            _ => (vx, h - vy),
        }
    };
    let (ax, ay) = map(vx0, vy0);
    let (bx, by) = map(vx1, vy1);
    (ax.min(bx), ay.min(by), (ax - bx).abs(), (ay - by).abs())
}

fn number(value: &Value) -> f32 {
    value.as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32
}
use serde_json::Value;

fn overlay_stream(annotations: &[&Annotation], bounds: (f32, f32, f32, f32), rot: i64) -> Vec<u8> {
    let (llx, lly, urx, ury) = bounds;
    let (w, h) = (urx - llx, ury - lly);
    let mut out = String::new();
    for item in annotations {
        let (r, g, b) = hex_to_rgb(&item.color);
        let rects = item.rects.as_array().cloned().unwrap_or_default();
        for rect in rects {
            let (nx, ny, nw, nh) = (
                number(&rect["x"]),
                number(&rect["y"]),
                number(&rect["width"]),
                number(&rect["height"]),
            );
            let (x, y, rw, rh) = map_rect(rot, w, h, nx, ny, nw, nh);
            let (x, y) = (x + llx, y + lly);
            if item.annotation_type == "sentence" {
                out.push_str(&format!(
                    "q /{GS_FILL} gs {r:.3} {g:.3} {b:.3} rg {x:.3} {y:.3} {rw:.3} {rh:.3} re f Q\n"
                ));
            } else {
                out.push_str(&format!(
                    "q /{GS_BOX} gs {r:.3} {g:.3} {b:.3} rg {r:.3} {g:.3} {b:.3} RG 2 w {x:.3} {y:.3} {rw:.3} {rh:.3} re B Q\n"
                ));
            }
        }
    }
    out.into_bytes()
}

fn resolve_dict(doc: &Document, object: &Object) -> Option<Dictionary> {
    match object {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    }
}

pub fn build_annotated_pdf(source: &[u8], annotations: &[Annotation]) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|e| format!("This PDF is password protected: {e}"))?;
    }

    let mut by_page: BTreeMap<u32, Vec<&Annotation>> = BTreeMap::new();
    for item in annotations {
        if item.page_number >= 1 {
            by_page.entry(item.page_number as u32).or_default().push(item);
        }
    }

    let pages = doc.get_pages();
    for (page_number, items) in &by_page {
        let Some(&page_id) = pages.get(page_number) else { continue };
        let bounds = media_box(&doc, page_id);
        let rot = rotation(&doc, page_id);
        let overlay = overlay_stream(items, bounds, rot);
        if overlay.is_empty() {
            continue;
        }

        // Resources: copy the (possibly inherited / shared) dictionary onto the page
        // and add our two transparency states.
        let mut resources = inherited(&doc, page_id, b"Resources")
            .and_then(|o| resolve_dict(&doc, &o))
            .unwrap_or_default();
        let mut ext_g_state = resources
            .get(b"ExtGState")
            .ok()
            .and_then(|o| resolve_dict(&doc, o))
            .unwrap_or_default();
        ext_g_state.set(GS_FILL, dictionary! { "Type" => "ExtGState", "ca" => 0.34_f32 });
        ext_g_state.set(GS_BOX, dictionary! { "Type" => "ExtGState", "ca" => 0.10_f32, "CA" => 0.96_f32 });
        resources.set("ExtGState", Object::Dictionary(ext_g_state));

        // Contents: wrap the original in q … Q so unbalanced graphics state cannot
        // leak into the overlay, then append the overlay stream.
        let existing = doc
            .get_dictionary(page_id)
            .map_err(|e| e.to_string())?
            .get(b"Contents")
            .ok()
            .cloned();
        let mut parts: Vec<Object> = Vec::new();
        let q = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        parts.push(Object::Reference(q));
        match existing {
            Some(Object::Reference(id)) => match doc.get_object(id) {
                Ok(Object::Array(array)) => parts.extend(array.clone()),
                _ => parts.push(Object::Reference(id)),
            },
            Some(Object::Array(array)) => parts.extend(array),
            _ => {}
        }
        let big_q = doc.add_object(Stream::new(Dictionary::new(), b"Q\n".to_vec()));
        let overlay_id = doc.add_object(Stream::new(Dictionary::new(), overlay));
        parts.push(Object::Reference(big_q));
        parts.push(Object::Reference(overlay_id));

        let page = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
        page.set("Resources", Object::Dictionary(resources));
        page.set("Contents", Object::Array(parts));
    }

    let mut output = Vec::new();
    doc.save_to(&mut output).map_err(|e| e.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_mapping_round_trips_for_each_rotation() {
        // A 200 x 100 page. A box in the top-left quarter of the rendered page.
        let (x, y, w, h) = map_rect(0, 200.0, 100.0, 0.0, 0.0, 0.5, 0.5);
        assert_eq!((x, y, w, h), (0.0, 50.0, 100.0, 50.0));
        // Rotated 90°: rendered page is 100 wide x 200 tall.
        let (x, y, w, h) = map_rect(90, 200.0, 100.0, 0.0, 0.0, 0.5, 0.5);
        assert_eq!((x, y, w, h), (0.0, 0.0, 100.0, 50.0));
        let (x, y, ..) = map_rect(180, 200.0, 100.0, 0.0, 0.0, 0.5, 0.5);
        assert_eq!((x, y), (100.0, 0.0));
        let (x, y, ..) = map_rect(270, 200.0, 100.0, 0.0, 0.0, 0.5, 0.5);
        assert_eq!((x, y), (100.0, 50.0));
    }

    #[test]
    fn colours_parse() {
        let (r, g, b) = hex_to_rgb("#ff0000");
        assert_eq!((r, g, b), (1.0, 0.0, 0.0));
        let (r, ..) = hex_to_rgb("garbage");
        assert!(r > 0.9);
    }
}
