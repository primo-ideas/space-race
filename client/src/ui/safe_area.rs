//! The part of the screen nothing that matters may be drawn outside of. A phone's screen is not a
//! rectangle: held in landscape, an iPhone's notch or island cuts into one side, its corners are
//! rounded off, and the bar that sends it home runs along the bottom edge, where a thumb that
//! slides down goes home instead of steering. The page fills all of it (`viewport-fit=cover`), so
//! the race and the menus' backdrop reach into the corners, and the browser says, through CSS's
//! `env(safe-area-inset-*)`, how far in from each edge everything that is read or pressed must
//! stay.
//!
//! Two kinds of interface root follow it. A [`SafeInset`] root has its edges moved onto the safe
//! area's, so everything pinned to its corners -- the HUD, the touch controls -- comes in with
//! them. A [`SafePadding`] root adds the insets to its own padding, so the screen it lays out keeps
//! clear of them while its background still fills the window.

use bevy::prelude::*;
use bevy::ui::UiSystems;

pub struct SafeAreaPlugin {
    /// Insets to use instead of the browser's, to look at the interface laid out around a notch
    /// on a screen that has none (`--safe-area`, `?safe_area=`).
    pub fixed: Option<SafeArea>,
}

impl Plugin for SafeAreaPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.fixed.unwrap_or_default())
            .add_systems(PostUpdate, fit_to_safe_area.before(UiSystems::Prepare));
        if self.fixed.is_none() {
            app.add_systems(PreUpdate, follow_the_page);
        }
    }
}

/// How far in from each edge of the window the interface keeps, in logical pixels: what a phone's
/// notch, rounded corners and home bar take from its screen. Nothing anywhere else.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct SafeArea {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl SafeArea {
    /// The safe part of a window `size` logical pixels big, in interface units at `scale`.
    pub fn rect(self, size: Vec2, scale: f32) -> Rect {
        let scale = scale.max(0.1);
        let min = Vec2::new(self.left, self.top) / scale;
        let max = (size - Vec2::new(self.right, self.bottom)) / scale;
        Rect::from_corners(min, max.max(min))
    }
}

impl std::str::FromStr for SafeArea {
    type Err = String;

    /// `TOP,RIGHT,BOTTOM,LEFT`, the order CSS writes padding in, such as `0,47,21,47` for an
    /// iPhone held in landscape.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let complaint = || {
            format!(
                "expected TOP,RIGHT,BOTTOM,LEFT, none negative, such as 0,47,21,47, not {value}"
            )
        };
        let sides: Vec<f32> = value
            .split(',')
            .map(|side| side.trim().parse::<f32>())
            .collect::<Result<_, _>>()
            .map_err(|_| complaint())?;
        match sides[..] {
            [top, right, bottom, left] if sides.iter().all(|side| *side >= 0.0) => Ok(Self {
                top,
                right,
                bottom,
                left,
            }),
            _ => Err(complaint()),
        }
    }
}

/// An interface root laid over the safe area rather than the window: its edges sit on the safe
/// area's, and whatever is pinned to its corners comes in with them.
#[derive(Component)]
pub struct SafeInset;

/// An interface root filling the window, its background and all, whose padding grows by the
/// insets so that what it lays out keeps clear of them. Its own padding is remembered the first
/// time it is seen.
#[derive(Component, Default)]
pub struct SafePadding {
    own: Option<UiRect>,
}

fn fit_to_safe_area(
    area: Res<SafeArea>,
    scale: Res<UiScale>,
    mut insets: Query<&mut Node, (With<SafeInset>, Without<SafePadding>)>,
    mut padded: Query<(&mut SafePadding, &mut Node), Without<SafeInset>>,
) {
    let scale = scale.0.max(0.1);
    let side = |inset: f32| inset / scale;

    for mut node in &mut insets {
        let edges = [
            px(side(area.left)),
            px(side(area.right)),
            px(side(area.top)),
            px(side(area.bottom)),
        ];
        if node.position_type != PositionType::Absolute
            || [node.left, node.right, node.top, node.bottom] != edges
            || node.width != Val::Auto
            || node.height != Val::Auto
        {
            node.position_type = PositionType::Absolute;
            [node.left, node.right, node.top, node.bottom] = edges;
            node.width = Val::Auto;
            node.height = Val::Auto;
        }
    }

    for (mut padding, mut node) in &mut padded {
        let own = *padding.own.get_or_insert(node.padding);
        let grown = UiRect {
            left: grow(own.left, side(area.left)),
            right: grow(own.right, side(area.right)),
            top: grow(own.top, side(area.top)),
            bottom: grow(own.bottom, side(area.bottom)),
        };
        if node.padding != grown {
            node.padding = grown;
        }
    }
}

/// A padding grown by `inset` interface units. Every padding a root has is in pixels; anything
/// else gives way to the inset.
fn grow(own: Val, inset: f32) -> Val {
    match own {
        Val::Px(own) => px(own + inset),
        _ if inset > 0.0 => px(inset),
        own => own,
    }
}

/// Reads the insets from the page twice a second: they change when the phone is turned over from
/// one side to the other, which resizes nothing, and when the page goes full screen.
fn follow_the_page(mut area: ResMut<SafeArea>, time: Res<Time<Real>>, mut next: Local<f64>) {
    let now = time.elapsed_secs_f64();
    if now < *next {
        return;
    }
    *next = now + 0.5;
    if let Some(read) = from_the_page() {
        area.set_if_neq(read);
    }
}

/// The insets as the browser gives them: `index.html` holds an invisible element padded by
/// `env(safe-area-inset-*)`, and its computed padding is the insets in CSS pixels, which are the
/// window's logical pixels. Nothing natively, where no window has a notch.
fn from_the_page() -> Option<SafeArea> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let probe = window.document()?.get_element_by_id("safe-area")?;
        let style = window.get_computed_style(&probe).ok()??;
        let side = |name: &str| {
            style
                .get_property_value(name)
                .ok()
                .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
                .unwrap_or(0.0)
        };
        Some(SafeArea {
            top: side("padding-top"),
            right: side("padding-right"),
            bottom: side("padding-bottom"),
            left: side("padding-left"),
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_are_read_in_the_order_css_writes_them() {
        let area: SafeArea = "0, 47,21,47.5".parse().unwrap();
        assert_eq!(
            area,
            SafeArea {
                top: 0.0,
                right: 47.0,
                bottom: 21.0,
                left: 47.5,
            }
        );
        assert!("0,47,21".parse::<SafeArea>().is_err());
        assert!("0,-1,0,0".parse::<SafeArea>().is_err());
        assert!("a,b,c,d".parse::<SafeArea>().is_err());
    }

    /// The safe rectangle is in interface units, so a touch divided by the scale meets it.
    #[test]
    fn the_safe_rectangle_is_in_interface_units() {
        let area = SafeArea {
            top: 0.0,
            right: 40.0,
            bottom: 20.0,
            left: 60.0,
        };
        let rect = area.rect(Vec2::new(840.0, 400.0), 2.0);
        assert_eq!(rect.min, Vec2::new(30.0, 0.0));
        assert_eq!(rect.max, Vec2::new(400.0, 190.0));
        // Insets wider than the window leave nothing, rather than a rectangle inside out.
        let squeezed = SafeArea {
            left: 500.0,
            right: 500.0,
            ..default()
        };
        assert_eq!(squeezed.rect(Vec2::new(840.0, 400.0), 1.0).width(), 0.0);
    }

    #[test]
    fn a_padding_grows_by_the_inset() {
        assert_eq!(grow(px(20), 47.0), px(67));
        assert_eq!(grow(Val::Auto, 0.0), Val::Auto);
        assert_eq!(grow(Val::Auto, 10.0), px(10));
    }
}
