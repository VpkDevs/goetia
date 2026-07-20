//! Room-grammar assembler. Templates occupy rectangles on an integer cell
//! grid and expose doors on their edges; the assembler grows a connected
//! layout door-by-door until the target room count is reached.

use goetia_core::Pcg32;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Side {
    North, // +y
    South, // -y
    East,  // +x
    West,  // -x
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::North => Side::South,
            Side::South => Side::North,
            Side::East => Side::West,
            Side::West => Side::East,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Door {
    pub side: Side,
    /// Cell offset along the wall (0-based from the wall's min corner).
    pub offset: u32,
}

/// Authored room template (RON). Sizes are in layout cells; the game decides
/// how big a cell is in world units.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomTemplate {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub doors: Vec<Door>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_weight")]
    pub weight: f32,
}

fn default_weight() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RealmGrammar {
    /// Template name the layout starts from.
    pub start: String,
    /// Stop growing once this many rooms are placed.
    pub target_rooms: u32,
    /// Allowed tag adjacencies (a, b) — symmetric. Empty = everything connects.
    #[serde(default)]
    pub allow: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacedRoom {
    pub template: usize,
    /// Min-corner cell position.
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RealmLayout {
    pub rooms: Vec<PlacedRoom>,
    /// (room a, room b, door cell) — the corridor/passage between them.
    pub connections: Vec<(usize, usize, (i32, i32))>,
}

impl RealmLayout {
    /// Stable fingerprint for determinism tests.
    pub fn hash(&self) -> u64 {
        let mut h = goetia_core::StateHasher::new();
        for r in &self.rooms {
            h.write_u64(r.template as u64);
            h.write_i32(r.x);
            h.write_i32(r.y);
        }
        for (a, b, (x, y)) in &self.connections {
            h.write_u64(*a as u64);
            h.write_u64(*b as u64);
            h.write_i32(*x);
            h.write_i32(*y);
        }
        h.finish()
    }
}

struct OpenDoor {
    room: usize,
    door: Door,
    /// Cell just outside the wall, where the next room must abut.
    exit_cell: (i32, i32),
}

fn door_cells(t: &RoomTemplate, x: i32, y: i32, d: Door) -> ((i32, i32), (i32, i32)) {
    // (cell inside room at the door, cell just outside)
    match d.side {
        Side::North => (
            (x + d.offset as i32, y + t.height as i32 - 1),
            (x + d.offset as i32, y + t.height as i32),
        ),
        Side::South => ((x + d.offset as i32, y), (x + d.offset as i32, y - 1)),
        Side::East => (
            (x + t.width as i32 - 1, y + d.offset as i32),
            (x + t.width as i32, y + d.offset as i32),
        ),
        Side::West => ((x, y + d.offset as i32), (x - 1, y + d.offset as i32)),
    }
}

fn tags_allowed(g: &RealmGrammar, a: &RoomTemplate, b: &RoomTemplate) -> bool {
    if g.allow.is_empty() {
        return true;
    }
    let pair_ok = |ta: &str, tb: &str| {
        g.allow
            .iter()
            .any(|(x, y)| (x == ta && y == tb) || (x == tb && y == ta))
    };
    a.tags
        .iter()
        .any(|ta| b.tags.iter().any(|tb| pair_ok(ta, tb)))
        || (a.tags.is_empty() || b.tags.is_empty())
}

/// Assemble a connected realm. Deterministic in (templates, grammar, rng
/// state). Returns None only if the start template is missing.
pub fn assemble(
    templates: &[RoomTemplate],
    grammar: &RealmGrammar,
    rng: &mut Pcg32,
) -> Option<RealmLayout> {
    let start_idx = templates.iter().position(|t| t.name == grammar.start)?;
    let mut layout = RealmLayout::default();
    let mut occupied: HashSet<(i32, i32)> = HashSet::new();
    let mut open: Vec<OpenDoor> = Vec::new();

    let place = |layout: &mut RealmLayout,
                 occupied: &mut HashSet<(i32, i32)>,
                 open: &mut Vec<OpenDoor>,
                 ti: usize,
                 x: i32,
                 y: i32| {
        let t = &templates[ti];
        for cx in x..x + t.width as i32 {
            for cy in y..y + t.height as i32 {
                occupied.insert((cx, cy));
            }
        }
        let idx = layout.rooms.len();
        layout.rooms.push(PlacedRoom { template: ti, x, y });
        for &d in &t.doors {
            let (_, exit) = door_cells(t, x, y, d);
            open.push(OpenDoor {
                room: idx,
                door: d,
                exit_cell: exit,
            });
        }
        idx
    };

    place(&mut layout, &mut occupied, &mut open, start_idx, 0, 0);

    let mut attempts = 0u32;
    let max_attempts = grammar.target_rooms * 64;
    while (layout.rooms.len() as u32) < grammar.target_rooms && !open.is_empty() {
        attempts += 1;
        if attempts > max_attempts {
            break;
        }
        // Deterministic draw order: pick an open door, then a template.
        let di = rng.range_u32(open.len() as u32) as usize;
        let od = &open[di];
        let host_t = &templates[layout.rooms[od.room].template];

        // Candidate templates that have a door on the opposite side and pass
        // the tag grammar. Weighted pick.
        let need = od.door.side.opposite();
        let mut cand: Vec<(usize, Door, f32)> = Vec::new();
        for (ti, t) in templates.iter().enumerate() {
            if !tags_allowed(grammar, host_t, t) {
                continue;
            }
            for &d in &t.doors {
                if d.side == need {
                    cand.push((ti, d, t.weight));
                }
            }
        }
        if cand.is_empty() {
            open.swap_remove(di);
            continue;
        }
        let weights: Vec<f32> = cand.iter().map(|c| c.2).collect();
        let pick = rng.weighted_index(&weights).unwrap();
        let (ti, tdoor, _) = cand[pick];
        let t = &templates[ti];

        // Position the new room so its door's inside-cell == exit cell.
        let (ex, ey) = od.exit_cell;
        let (nx, ny) = match need {
            Side::South => (ex - tdoor.offset as i32, ey),
            Side::North => (ex - tdoor.offset as i32, ey - t.height as i32 + 1),
            Side::West => (ex, ey - tdoor.offset as i32),
            Side::East => (ex - t.width as i32 + 1, ey - tdoor.offset as i32),
        };

        // Overlap check.
        let mut blocked = false;
        'outer: for cx in nx..nx + t.width as i32 {
            for cy in ny..ny + t.height as i32 {
                if occupied.contains(&(cx, cy)) {
                    blocked = true;
                    break 'outer;
                }
            }
        }
        if blocked {
            // Door stays open; a smaller template may fit on a later attempt.
            continue;
        }

        let host = od.room;
        let exit_cell = od.exit_cell;
        open.swap_remove(di);
        let new_idx = place(&mut layout, &mut occupied, &mut open, ti, nx, ny);
        // Remove the new room's door that we just used for the connection.
        open.retain(|o| {
            !(o.room == new_idx && o.door.side == tdoor.side && o.door.offset == tdoor.offset)
        });
        layout.connections.push((host, new_idx, exit_cell));
    }
    Some(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, w: u32, h: u32, doors: Vec<Door>) -> RoomTemplate {
        RoomTemplate {
            name: name.into(),
            width: w,
            height: h,
            doors,
            tags: vec![],
            weight: 1.0,
        }
    }

    fn templates() -> Vec<RoomTemplate> {
        vec![
            t(
                "hub",
                3,
                3,
                vec![
                    Door {
                        side: Side::North,
                        offset: 1,
                    },
                    Door {
                        side: Side::South,
                        offset: 1,
                    },
                    Door {
                        side: Side::East,
                        offset: 1,
                    },
                    Door {
                        side: Side::West,
                        offset: 1,
                    },
                ],
            ),
            t(
                "hall",
                2,
                4,
                vec![
                    Door {
                        side: Side::North,
                        offset: 0,
                    },
                    Door {
                        side: Side::South,
                        offset: 1,
                    },
                ],
            ),
            t(
                "cell",
                2,
                2,
                vec![Door {
                    side: Side::West,
                    offset: 0,
                }],
            ),
        ]
    }

    fn grammar(n: u32) -> RealmGrammar {
        RealmGrammar {
            start: "hub".into(),
            target_rooms: n,
            allow: vec![],
        }
    }

    #[test]
    fn same_seed_same_layout() {
        let ts = templates();
        let g = grammar(15);
        let a = assemble(&ts, &g, &mut Pcg32::new(99, 1)).unwrap();
        let b = assemble(&ts, &g, &mut Pcg32::new(99, 1)).unwrap();
        assert_eq!(a.hash(), b.hash());
        assert!(a.rooms.len() >= 5, "grew to {} rooms", a.rooms.len());
        let c = assemble(&ts, &g, &mut Pcg32::new(100, 1)).unwrap();
        assert_ne!(a.hash(), c.hash());
    }

    #[test]
    fn rooms_never_overlap() {
        let ts = templates();
        let g = grammar(20);
        let l = assemble(&ts, &g, &mut Pcg32::new(7, 1)).unwrap();
        let mut seen = std::collections::HashSet::new();
        for r in &l.rooms {
            let t = &ts[r.template];
            for x in r.x..r.x + t.width as i32 {
                for y in r.y..r.y + t.height as i32 {
                    assert!(seen.insert((x, y)), "overlap at {x},{y}");
                }
            }
        }
        // Connectivity: every room after the first appears in a connection.
        for i in 1..l.rooms.len() {
            assert!(l.connections.iter().any(|(a, b, _)| *a == i || *b == i));
        }
    }
}
