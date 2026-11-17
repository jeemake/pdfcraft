//! Smart fill: the outline of the enclosed region around a point of a rendered page, for an
//! area measurement made with one tap inside a room.
//!
//! The page is read as pixels, so scanned plans work as well as vector ones. Dark pixels are
//! walls; the region is the light pixels reachable from the seed. Gaps narrower than
//! `close_gaps` pixels (door openings) are closed first, by growing the walls, and the region
//! then grows back by as much so its edge sits on the walls again. Its outer outline is traced
//! along pixel edges, so text and furniture drawn inside a room don't count against its area.

use serde::Serialize;

/// Most pixels read: about a 6000 × 4000 raster.
pub const MAX_PIXELS: usize = 25_000_000;
/// Most gap closing, in pixels.
pub const MAX_CLOSE: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillOptions {
    /// A pixel at or below this luminance (0–255) is a wall.
    pub threshold: u8,
    /// Gaps up to about twice this many pixels wide are closed.
    pub close_gaps: u32,
    /// The outline is simplified to within this many pixels.
    pub tolerance: f64,
}
impl Default for FillOptions {
    fn default() -> Self {
        Self { threshold: 160, close_gaps: 2, tolerance: 0.75 }
    }
}

/// The region found: its outline in pixel coordinates (x right, y down, on pixel corners) and
/// how many pixels it covers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Region {
    pub outline: Vec<[f64; 2]>,
    pub pixels: usize,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FillError {
    #[error("the raster is empty, too large or its size doesn't match its pixels")]
    BadRaster,
    #[error("the point is outside the page")]
    Outside,
    #[error("the point is on a line; tap inside an enclosed area")]
    OnWall,
    #[error("the area isn't enclosed: it reaches the edge of the page; close its gaps or draw it point by point")]
    Open,
}

/// Luminance (0–255) of premultiplied RGBA pixels over white.
pub fn luminance(rgba: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let count = width.checked_mul(height)?;
    if count == 0 || count > MAX_PIXELS || rgba.len() != count.checked_mul(4)? {
        return None;
    }
    Some(
        rgba.chunks_exact(4)
            .map(|p| {
                let (r, g, b, a) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]), u32::from(p[3]));
                // Premultiplied over white: c + (255 - a).
                let over = |c: u32| (c + 255u32.saturating_sub(a)).min(255);
                ((over(r) * 299 + over(g) * 587 + over(b) * 114) / 1000) as u8
            })
            .collect(),
    )
}

/// The enclosed region of `lum` (width × height luminance) around `seed` (pixel x, y).
pub fn fill(lum: &[u8], width: usize, height: usize, seed: [f64; 2], options: &FillOptions) -> Result<Region, FillError> {
    let count = width.checked_mul(height).ok_or(FillError::BadRaster)?;
    if count == 0 || count > MAX_PIXELS || lum.len() != count {
        return Err(FillError::BadRaster);
    }
    if !seed[0].is_finite() || !seed[1].is_finite() || seed[0] < 0.0 || seed[1] < 0.0 || seed[0] >= width as f64 || seed[1] >= height as f64 {
        return Err(FillError::Outside);
    }
    let (sx, sy) = (seed[0] as usize, seed[1] as usize);
    let wall: Vec<bool> = lum.iter().map(|&l| l <= options.threshold).collect();
    if wall.get(sy * width + sx).copied().unwrap_or(true) {
        return Err(FillError::OnWall);
    }
    let r = options.close_gaps.min(MAX_CLOSE) as usize;
    let grown = if r > 0 { dilate(&wall, width, height, r) } else { wall.clone() };
    // The seed may sit within `r` of a wall: start from the nearest free pixel of the grown walls.
    let start = nearest_free(&grown, width, height, sx, sy, r).ok_or(FillError::OnWall)?;
    let mut inside = vec![false; count];
    let mut stack = vec![start];
    let mut touches_edge = false;
    while let Some(i) = stack.pop() {
        if inside.get(i).copied().unwrap_or(true) || grown.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(c) = inside.get_mut(i) {
            *c = true;
        }
        let (x, y) = (i % width, i / width);
        if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
            touches_edge = true;
        }
        if x > 0 {
            stack.push(i - 1);
        }
        if x + 1 < width {
            stack.push(i + 1);
        }
        if y > 0 {
            stack.push(i - width);
        }
        if y + 1 < height {
            stack.push(i + width);
        }
    }
    if touches_edge {
        return Err(FillError::Open);
    }
    // Grow back over the gap closing, but never onto a wall.
    let region: Vec<bool> = if r > 0 { dilate(&inside, width, height, r).iter().zip(&wall).map(|(&g, &w)| g && !w).collect() } else { inside };
    let pixels = region.iter().filter(|&&b| b).count();
    let outline = trace(&region, width, height).ok_or(FillError::OnWall)?;
    let outline = simplify(&outline, options.tolerance.clamp(0.0, 16.0));
    Ok(Region { outline, pixels })
}

/// The pixel within `r` of (x, y) that `blocked` leaves free, nearest first.
fn nearest_free(blocked: &[bool], width: usize, height: usize, x: usize, y: usize, r: usize) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    for dy in -(r as i64)..=(r as i64) {
        for dx in -(r as i64)..=(r as i64) {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                continue;
            }
            let i = ny as usize * width + nx as usize;
            let d = (dx * dx + dy * dy) as usize;
            if !blocked.get(i).copied().unwrap_or(true) && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// `mask` grown by a (2r + 1)-pixel square, as two running-count passes.
fn dilate(mask: &[bool], width: usize, height: usize, r: usize) -> Vec<bool> {
    let mut rows = vec![false; mask.len()];
    for y in 0..height {
        let row = y * width;
        let mut count = 0usize;
        for x in 0..(width + r) {
            if x < width && mask.get(row + x).copied().unwrap_or(false) {
                count += 1;
            }
            if x > 2 * r && x - 2 * r - 1 < width && mask.get(row + x - 2 * r - 1).copied().unwrap_or(false) {
                count = count.saturating_sub(1);
            }
            if x >= r
                && let Some(out) = rows.get_mut(row + x - r)
            {
                *out = count > 0;
            }
        }
    }
    let mut out = vec![false; mask.len()];
    for x in 0..width {
        let mut count = 0usize;
        for y in 0..(height + r) {
            if y < height && rows.get(y * width + x).copied().unwrap_or(false) {
                count += 1;
            }
            if y > 2 * r && y - 2 * r - 1 < height && rows.get((y - 2 * r - 1) * width + x).copied().unwrap_or(false) {
                count = count.saturating_sub(1);
            }
            if y >= r
                && let Some(o) = out.get_mut((y - r) * width + x)
            {
                *o = count > 0;
            }
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Right,
    Down,
    Left,
    Up,
}
impl Dir {
    fn right_turn(self) -> Self {
        match self {
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
            Self::Up => Self::Right,
        }
    }
    fn left_turn(self) -> Self {
        self.right_turn().right_turn().right_turn()
    }
    fn step(self) -> (i64, i64) {
        match self {
            Self::Right => (1, 0),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Up => (0, -1),
        }
    }
}

/// The outer outline of `region` along pixel edges, clockwise on screen, from its top-left
/// pixel: corners only (straight runs merged).
fn trace(region: &[bool], width: usize, height: usize) -> Option<Vec<[f64; 2]>> {
    let at = |x: i64, y: i64| {
        x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && region.get(y as usize * width + x as usize).copied().unwrap_or(false)
    };
    let first = region.iter().position(|&b| b)?;
    let start = ((first % width) as i64, (first / width) as i64);
    // An edge leaving corner v in direction d, with the region on its right and not on its left.
    let edge = |(vx, vy): (i64, i64), d: Dir| match d {
        Dir::Right => at(vx, vy) && !at(vx, vy - 1),
        Dir::Down => at(vx - 1, vy) && !at(vx, vy),
        Dir::Left => at(vx - 1, vy - 1) && !at(vx - 1, vy),
        Dir::Up => at(vx, vy - 1) && !at(vx - 1, vy - 1),
    };
    let mut v = start;
    let mut d = Dir::Right;
    let mut corners = vec![[start.0 as f64, start.1 as f64]];
    let limit = width.saturating_mul(height).saturating_mul(4).saturating_add(8);
    for _ in 0..limit {
        let (dx, dy) = d.step();
        v = (v.0 + dx, v.1 + dy);
        let next = [d.right_turn(), d, d.left_turn()].into_iter().find(|&n| edge(v, n))?;
        if v == start && next == Dir::Right {
            return Some(corners);
        }
        if next != d {
            corners.push([v.0 as f64, v.1 as f64]);
        }
        d = next;
    }
    None
}

/// Douglas–Peucker over a closed outline.
pub fn simplify(points: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if points.len() < 4 || tolerance <= 0.0 {
        return points.to_vec();
    }
    // Split the ring at the point farthest from the first, and simplify each half.
    let first = points[0];
    let far = (1..points.len()).max_by(|&a, &b| dist2(points[a], first).total_cmp(&dist2(points[b], first))).unwrap_or(1);
    let mut keep = vec![false; points.len() + 1];
    keep[0] = true;
    keep[far] = true;
    keep[points.len()] = true;
    let ring: Vec<[f64; 2]> = points.iter().copied().chain(std::iter::once(first)).collect();
    let mut stack = vec![(0, far), (far, points.len())];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (ring[a], ring[b]);
        let mut worst = (0.0, a);
        for (i, p) in ring.iter().enumerate().take(b).skip(a + 1) {
            let d = segment_distance(*p, pa, pb);
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    points.iter().enumerate().filter(|(i, _)| keep[*i]).map(|(_, p)| *p).collect()
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        return dist2(p, a).sqrt();
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    dist2(p, [a[0] + t * dx, a[1] + t * dy]).sqrt()
}

/// The area of a closed outline (shoelace), in square pixels.
pub fn area(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        sum += a[0] * b[1] - b[0] * a[1];
    }
    (sum / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A white raster with black rectangles (x0, y0, x1, y1, exclusive ends).
    fn raster(w: usize, h: usize, walls: &[(usize, usize, usize, usize)]) -> Vec<u8> {
        let mut out = vec![255u8; w * h];
        for &(x0, y0, x1, y1) in walls {
            for y in y0..y1 {
                for x in x0..x1 {
                    out[y * w + x] = 0;
                }
            }
        }
        out
    }

    /// A 100 × 80 room drawn with 2-pixel walls, inside a 200 × 200 page.
    fn room(door: bool) -> Vec<u8> {
        let mut walls = vec![(20, 20, 122, 22), (20, 100, 122, 102), (20, 20, 22, 102), (120, 20, 122, 102)];
        if door {
            // A 3-pixel opening in the bottom wall.
            walls[1] = (20, 100, 60, 102);
            walls.push((63, 100, 122, 102));
        }
        raster(200, 200, &walls)
    }

    #[test]
    fn a_closed_room_is_filled_to_its_walls() {
        let lum = room(false);
        let r = fill(&lum, 200, 200, [70.0, 60.0], &FillOptions { close_gaps: 0, ..Default::default() }).unwrap();
        // Interior: x 22..120, y 22..100 = 98 × 78.
        assert_eq!(r.pixels, 98 * 78);
        assert_eq!(r.outline, vec![[22.0, 22.0], [120.0, 22.0], [120.0, 100.0], [22.0, 100.0]]);
        assert_eq!(area(&r.outline), (98 * 78) as f64);
    }

    #[test]
    fn text_inside_the_room_does_not_reduce_its_area() {
        let mut lum = room(false);
        for y in 50..60 {
            for x in 60..80 {
                lum[y * 200 + x] = 0;
            }
        }
        let r = fill(&lum, 200, 200, [30.0, 30.0], &FillOptions { close_gaps: 0, ..Default::default() }).unwrap();
        assert_eq!(area(&r.outline), (98 * 78) as f64);
    }

    #[test]
    fn a_door_opening_leaks_unless_gaps_are_closed() {
        let lum = room(true);
        let open = fill(&lum, 200, 200, [70.0, 60.0], &FillOptions { close_gaps: 0, ..Default::default() });
        assert_eq!(open, Err(FillError::Open));
        let closed = fill(&lum, 200, 200, [70.0, 60.0], &FillOptions { close_gaps: 2, ..Default::default() }).unwrap();
        let a = area(&closed.outline);
        // The room, give or take the doorway's few pixels.
        assert!((a - (98.0 * 78.0)).abs() < 3.0 * 4.0, "area {a}");
    }

    #[test]
    fn a_tap_on_a_wall_or_off_the_page_is_refused() {
        let lum = room(false);
        assert_eq!(fill(&lum, 200, 200, [21.0, 50.0], &FillOptions { close_gaps: 0, ..Default::default() }), Err(FillError::OnWall));
        assert_eq!(fill(&lum, 200, 200, [-1.0, 50.0], &FillOptions::default()), Err(FillError::Outside));
        assert_eq!(fill(&lum, 200, 200, [f64::NAN, 50.0], &FillOptions::default()), Err(FillError::Outside));
        assert_eq!(fill(&lum, 200, 200, [250.0, 50.0], &FillOptions::default()), Err(FillError::Outside));
        assert_eq!(fill(&lum, 199, 200, [5.0, 5.0], &FillOptions::default()), Err(FillError::BadRaster));
        assert_eq!(fill(&[], 0, 0, [0.0, 0.0], &FillOptions::default()), Err(FillError::BadRaster));
    }

    #[test]
    fn an_l_shaped_room_keeps_its_corners() {
        let w = 100;
        let mut lum = vec![0u8; w * w];
        for y in 10..90 {
            for x in 10..90 {
                if !(x >= 50 && y >= 50) {
                    lum[y * w + x] = 255;
                }
            }
        }
        let r = fill(&lum, w, w, [20.0, 20.0], &FillOptions { close_gaps: 0, ..Default::default() }).unwrap();
        assert_eq!(r.outline.len(), 6);
        assert_eq!(area(&r.outline), (80 * 80 - 40 * 40) as f64);
    }

    #[test]
    fn a_diagonal_wall_simplifies_to_few_points() {
        let w = 120;
        let mut lum = vec![0u8; w * w];
        for y in 10..110 {
            for x in 10..110 {
                if x <= y {
                    lum[y * w + x] = 255;
                }
            }
        }
        let r = fill(&lum, w, w, [15.0, 100.0], &FillOptions { close_gaps: 0, tolerance: 1.0, ..Default::default() }).unwrap();
        assert!(r.outline.len() <= 6, "{:?}", r.outline);
        let exact = r.pixels as f64;
        assert!((area(&r.outline) - exact).abs() / exact < 0.02);
    }

    #[test]
    fn luminance_reads_premultiplied_pixels_over_white() {
        let rgba = [0, 0, 0, 255, 0, 0, 0, 0, 255, 255, 255, 255];
        assert_eq!(luminance(&rgba, 3, 1), Some(vec![0, 255, 255]));
        assert_eq!(luminance(&rgba, 2, 1), None);
    }
}
