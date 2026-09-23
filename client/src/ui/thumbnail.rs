//! A track seen from above, drawn into an image on the CPU: the road with neon edges and a soft
//! glow, and the start line. Menus show it wherever a track is named.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use space_race_sim::track::{Track, TrackPoint};

/// Twice the size it is usually displayed at, so it stays sharp on large screens.
const WIDTH: u32 = 400;
const HEIGHT: u32 = 250;
/// Room left around the track, in pixels, for the glow.
const MARGIN: f32 = 22.0;
/// Width of the neon edge line, and how far its glow reaches, in pixels.
const EDGE_WIDTH: f32 = 2.2;
const GLOW_REACH: f32 = 9.0;
/// The road is drawn at least this wide, in pixels, so a narrow road stays visible.
const MIN_HALF_WIDTH: f32 = 3.0;

const ROAD: [f32; 3] = [0.10, 0.11, 0.14];
const NEON: [f32; 3] = [0.05, 0.6, 1.0];
const START_LINE: [f32; 3] = [0.95, 0.95, 0.95];

pub fn render(track: &Track) -> Image {
    let points = track.points();
    // The widest the road ever is, which the frame has to hold.
    let widest = track.half_width();
    let (min, max) = points.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(min, max), point| (min.min(point.position), max.max(point.position)),
    );
    let min = min - widest;
    let max = max + widest;

    let available = Vec2::new(WIDTH as f32, HEIGHT as f32) - 2.0 * MARGIN;
    let scale = (available / (max - min)).min_element();
    let offset = (Vec2::new(WIDTH as f32, HEIGHT as f32) - (max - min) * scale) / 2.0;
    // Image rows grow downward, the track's `y` upward.
    let to_pixels = |position: Vec2| {
        let local = (position - min) * scale + offset;
        Vec2::new(local.x, HEIGHT as f32 - local.y)
    };
    // The road is drawn as wide as it is at each point, so a bottleneck shows as one.
    let road_half_width = |point: &TrackPoint| (point.half_width * scale).max(MIN_HALF_WIDTH);

    // Distance from each pixel center to the edge of the road, negative on it, only computed near
    // the road.
    let mut from_edges = vec![f32::MAX; (WIDTH * HEIGHT) as usize];
    let reach = (widest * scale).max(MIN_HALF_WIDTH) + GLOW_REACH + EDGE_WIDTH;
    for index in 0..points.len() {
        let (from, to) = (&points[index], &points[(index + 1) % points.len()]);
        let (a, b) = (to_pixels(from.position), to_pixels(to.position));
        let (half_a, half_b) = (road_half_width(from), road_half_width(to));
        let low = (a.min(b) - reach).max(Vec2::ZERO);
        let high = (a.max(b) + reach).min(Vec2::new(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0));
        for y in low.y as u32..=high.y as u32 {
            for x in low.x as u32..=high.x as u32 {
                let pixel = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let (distance, t) = distance_to_segment(pixel, a, b);
                let from_edge = distance - (half_a + (half_b - half_a) * t);
                let slot = &mut from_edges[(y * WIDTH + x) as usize];
                *slot = slot.min(from_edge);
            }
        }
    }

    let start = to_pixels(points[0].position);
    let start_direction = Vec2::new(points[0].direction.x, -points[0].direction.y);
    let start_half_width = road_half_width(&points[0]);

    let mut data = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let from_edge = from_edges[(y * WIDTH + x) as usize];
            let pixel = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let color = shade(from_edge, start_half_width, pixel, start, start_direction);
            data.extend(color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }

    Image::new(
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Color and alpha of a pixel `from_edge` pixels outside the edge of the road, negative on it. The
/// start line crosses the road `start_half_width` pixels either side of `start`.
fn shade(
    from_edge: f32,
    start_half_width: f32,
    pixel: Vec2,
    start: Vec2,
    direction: Vec2,
) -> [f32; 4] {
    // Coverage of the road and of the edge line, antialiased over a pixel.
    let road = (0.5 - from_edge).clamp(0.0, 1.0);
    let edge = (1.0 - (from_edge.abs() - EDGE_WIDTH / 2.0)).clamp(0.0, 1.0);
    let glow = if from_edge > 0.0 {
        0.45 * (1.0 - from_edge / GLOW_REACH).clamp(0.0, 1.0).powi(2)
    } else {
        0.0
    };

    let along = (pixel - start).dot(direction).abs();
    let across = (pixel - start).perp_dot(direction).abs();
    let start_line = if across < start_half_width {
        (1.5 - along).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let mut color = ROAD;
    for (channel, (&neon, &line)) in color.iter_mut().zip(NEON.iter().zip(&START_LINE)) {
        *channel += (line - *channel) * start_line;
        *channel += (neon - *channel) * edge;
    }
    let alpha = road.max(edge).max(glow);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    // Outside the road, only the glow: pure neon, faded by its alpha.
    if road < 1.0 && edge < 1.0 {
        let solid = road.max(edge);
        let blend = if alpha > 0.0 { solid / alpha } else { 0.0 };
        for (channel, &neon) in color.iter_mut().zip(&NEON) {
            *channel = neon + (*channel - neon) * blend;
        }
    }
    [color[0], color[1], color[2], alpha]
}

/// Distance from `point` to the segment from `a` to `b`, and how far along it, from 0 to 1, the
/// closest point is.
fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let length_squared = ab.length_squared();
    let t = if length_squared > 0.0 {
        ((point - a).dot(ab) / length_squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point.distance(a + ab * t), t)
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Narrows, Segment, TrackDescription};

    use super::*;

    /// A flat oval 16 m wide, and whatever `narrows` pinch it.
    fn oval(narrows: Vec<Narrows>) -> Track {
        let turn = Segment::Turn {
            angle: 180.0,
            radius: 40.0,
            transition: 20.0,
            banking: 0.0,
        };
        Track::build(&TrackDescription {
            name: "Oval".into(),
            width: 16.0,
            segments: vec![
                Segment::Straight { length: 50.0 },
                turn,
                Segment::Straight { length: 100.0 },
                turn,
                Segment::Straight { length: 50.0 },
            ],
            narrows,
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// Pixels entirely covered by the dark road surface: opaque, and not neon or the start line.
    fn road_pixels(image: &Image) -> usize {
        let data = image.data.as_ref().unwrap();
        data.chunks(4)
            .filter(|pixel| pixel[3] == 255 && pixel[2] < 100)
            .count()
    }

    #[test]
    fn a_bottleneck_shows_on_the_thumbnail() {
        let open = road_pixels(&render(&oval(Vec::new())));
        // Half the width over 60 m of the straight opposite the line.
        let pinched = road_pixels(&render(&oval(vec![Narrows {
            at: 230.0,
            length: 60.0,
            width: 8.0,
            blend: 10.0,
        }])));
        assert!(open > 0);
        assert!(
            (pinched as f32) < open as f32 * 0.95,
            "{pinched} road pixels pinched, {open} open"
        );
    }
}
