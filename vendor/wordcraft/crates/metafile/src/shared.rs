//! The reader the application shares with Presentation (`deckcraft-metafile`), seen through this
//! crate's model, so Document draws metafiles with one parser instead of a second copy.
//!
//! That reader is a later version of this one: it also keeps clip regions, pattern brushes and
//! EMF+ records. Document's renderers draw what this crate's own reader produced, so the extra
//! detail is mapped back: clip regions are dropped and pattern fills (which this reader left
//! undrawn) are left out, as before.

use deckcraft_metafile as shared;

use crate::{Error, HAlign, Item, Picture, Seg, Text, VAlign};

/// Font height (cell height, logical units) of the default font, used for a height of 0.
const DEFAULT_FONT_HEIGHT: f64 = 16.0;
/// Face used when a font names none.
const DEFAULT_FONT_FACE: &str = "Arial";

pub(crate) fn parse(bytes: &[u8]) -> Result<Picture, Error> {
    let picture = shared::parse(bytes).map_err(|error| match error {
        shared::Error::NotMetafile => Error::NotMetafile,
        shared::Error::BadHeader => Error::BadHeader,
        shared::Error::NoRecords => Error::NoRecords,
    })?;
    let items = picture.items.into_iter().filter_map(item).collect();
    Ok(Picture { width: picture.width, height: picture.height, items })
}

pub(crate) fn is_metafile(bytes: &[u8]) -> bool {
    shared::is_metafile(bytes)
}

fn seg(s: shared::Seg) -> Seg {
    match s {
        shared::Seg::Move(x, y) => Seg::Move(x, y),
        shared::Seg::Line(x, y) => Seg::Line(x, y),
        shared::Seg::Cubic(a, b, c, d, e, f) => Seg::Cubic(a, b, c, d, e, f),
        shared::Seg::Close => Seg::Close,
    }
}

fn item(item: shared::Item) -> Option<Item> {
    Some(match item {
        shared::Item::Path { segs, fill, stroke, even_odd, .. } => Item::Path { segs: segs.into_iter().map(seg).collect(), fill, stroke, even_odd },
        shared::Item::Bitmap { rect, width, height, rgba, .. } => Item::Bitmap { rect, width, height, rgba },
        shared::Item::Pattern { .. } => return None,
        shared::Item::Text(text) => return self::text(*text),
    })
}

fn finite(v: f64) -> Option<f32> {
    let v = v as f32;
    v.is_finite().then_some(v)
}

/// A string placed by its text-space transform, as this crate's reference point, angle and sizes.
fn text(t: shared::Text) -> Option<Item> {
    let [a, b, c, d, e, f] = t.transform;
    // Lengths along the baseline and across it, per text unit.
    let (along, across) = (a.hypot(b), c.hypot(d));
    let height = if t.font.height == 0.0 { DEFAULT_FONT_HEIGHT } else { f64::from(t.font.height) };
    let size = finite(height.abs() * across).filter(|s| *s > 0.0)?;
    let angle = b.atan2(a);
    let dx = t.dx.iter().map(|d| finite((f64::from(*d) * along).copysign(f64::from(*d)))).collect::<Option<Vec<f32>>>()?;
    let family = t.font.family.trim();
    Some(Item::Text(Text {
        text: t.text,
        x: finite(e)?,
        y: finite(f)?,
        size,
        cell: height > 0.0,
        font: if family.is_empty() { DEFAULT_FONT_FACE.to_string() } else { family.to_string() },
        bold: t.font.weight >= 600,
        italic: t.font.italic,
        color: t.color,
        angle: finite(angle).unwrap_or(0.0),
        halign: match t.h_align {
            shared::HAlign::Left => HAlign::Left,
            shared::HAlign::Center => HAlign::Center,
            shared::HAlign::Right => HAlign::Right,
        },
        valign: match t.v_align {
            shared::VAlign::Top => VAlign::Top,
            shared::VAlign::Baseline => VAlign::Baseline,
            shared::VAlign::Bottom => VAlign::Bottom,
        },
        dx,
    }))
}
