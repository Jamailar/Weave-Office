use super::block::{Block, Blocks};
use super::parts::Part;
use super::{Arc, BTreeMap, BuildingBlock, Cell, Paragraph, Row, Table};

pub fn para_block(p: Paragraph) -> Arc<Block> {
    Arc::new(Block::Para(p))
}

/// The same blocks: the same content. The blocks are compared by address first — the
/// document is copy-on-write, so a block that changed was copied and has another address —
/// and a block that was copied but holds the same content is compared by value.
pub(crate) fn same_blocks(a: &Blocks, b: &Blocks) -> bool {
    same_blocks_except(a, b, &[])
}

/// Tail of the paragraph path in this child, empty for every other child.
fn child_path(path: &[u32], index: usize) -> &[u32] {
    match path.split_first() {
        Some((&first, tail)) if first as usize == index => tail,
        _ => &[],
    }
}

pub(crate) fn same_blocks_except(a: &Blocks, b: &Blocks, except: &[u32]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).enumerate().all(|(index, (x, y))| {
            if Arc::ptr_eq(x, y) {
                return true;
            }
            match (x.as_ref(), y.as_ref()) {
                (Block::Table(t), Block::Table(u)) => same_table(t, u, child_path(except, index)),
                (Block::Para(_), Block::Para(_)) if matches!(except, [i] if *i as usize == index) => true,
                _ => x == y,
            }
        })
}

/// The same table. Every field is listed, so a new one does not build until someone decides
/// whether a change to it counts.
fn same_table(a: &Table, b: &Table, except: &[u32]) -> bool {
    let Table { props, grid, rows, controls } = a;
    let Table { props: other_props, grid: other_grid, rows: other_rows, controls: other_controls } = b;
    props == other_props
        && grid == other_grid
        && controls == other_controls
        && rows.len() == other_rows.len()
        && rows.iter().zip(other_rows).enumerate().all(|(index, (x, y))| same_row(x, y, child_path(except, index)))
}

fn same_row(a: &Row, b: &Row, except: &[u32]) -> bool {
    let Row { props, cells, controls } = a;
    let Row { props: other_props, cells: other_cells, controls: other_controls } = b;
    props == other_props
        && controls == other_controls
        && cells.len() == other_cells.len()
        && cells.iter().zip(other_cells).enumerate().all(|(index, (x, y))| {
            let Cell { props, blocks, controls } = x;
            let Cell { props: other_props, blocks: other_blocks, controls: other_controls } = y;
            props == other_props && controls == other_controls && same_blocks_except(blocks, other_blocks, child_path(except, index))
        })
}

/// The same building blocks, their content compared by address first.
pub(crate) fn same_glossary(a: &[BuildingBlock], b: &[BuildingBlock]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            let BuildingBlock { name, gallery, category, description, guid, style, types, behaviors, blocks } = x;
            let BuildingBlock {
                name: other_name,
                gallery: other_gallery,
                category: other_category,
                description: other_description,
                guid: other_guid,
                style: other_style,
                types: other_types,
                behaviors: other_behaviors,
                blocks: other_blocks,
            } = y;
            name == other_name
                && gallery == other_gallery
                && category == other_category
                && description == other_description
                && guid == other_guid
                && style == other_style
                && types == other_types
                && behaviors == other_behaviors
                && same_blocks(blocks, other_blocks)
        })
}

/// The same parts: the same keys, and each part the same.
pub(crate) fn same_parts(a: &BTreeMap<u32, Part>, b: &BTreeMap<u32, Part>) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((k1, p1), (k2, p2))| k1 == k2 && same_part(p1, p2))
}

/// The same part: the same kind, text body and blocks. Every field is listed, so a new one
/// does not build until someone decides whether a change to it counts.
fn same_part(a: &Part, b: &Part) -> bool {
    let Part { kind, blocks, body } = a;
    let Part { kind: other_kind, blocks: other_blocks, body: other_body } = b;
    kind == other_kind && body == other_body && same_blocks(blocks, other_blocks)
}

/// The same bytes: the same content. The bytes are compared by address first — a changed
/// picture was copied, so it has another address — and a picture that was copied but holds
/// the same bytes is compared by value.
pub(crate) fn same_bytes(a: &BTreeMap<String, Arc<Vec<u8>>>, b: &BTreeMap<String, Arc<Vec<u8>>>) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((k1, v1), (k2, v2))| k1 == k2 && (Arc::ptr_eq(v1, v2) || **v1 == **v2))
}
