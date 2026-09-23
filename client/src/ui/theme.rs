//! The look of every screen: colors, fonts and text styles.
//!
//! Dark panels over the dark scene, one neon blue accent shared with the track, and a few status
//! colors that always mean the same thing: amber for a countdown, magenta for a race on, green for
//! results and finishes. The font is Saira, a variable font whose condensed italic heavy weights
//! give titles their motorsport feel.

use bevy::prelude::*;
use bevy::text::LetterSpacing;

pub const NEON: Color = Color::srgb(0.05, 0.6, 1.0);
pub const NEON_DIM: Color = Color::srgba(0.05, 0.6, 1.0, 0.35);
pub const NEON_FAINT: Color = Color::srgba(0.05, 0.6, 1.0, 0.07);

pub const TEXT: Color = Color::srgb(0.93, 0.95, 0.98);
pub const TEXT_DIM: Color = Color::srgb(0.60, 0.65, 0.74);
pub const TEXT_FAINT: Color = Color::srgb(0.40, 0.44, 0.52);
/// Text on a neon background.
pub const TEXT_ON_NEON: Color = Color::srgb(0.01, 0.05, 0.1);

pub const PANEL: Color = Color::srgba(0.025, 0.03, 0.05, 0.88);
pub const PANEL_BORDER: Color = Color::srgba(0.05, 0.6, 1.0, 0.22);
/// Rows and fields inside panels. Blending happens in linear space, where a few percent of white
/// already reads clearly on black.
pub const SURFACE: Color = Color::srgba(1.0, 1.0, 1.0, 0.012);
pub const SURFACE_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.03);
pub const BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);

pub const AMBER: Color = Color::srgb(1.0, 0.72, 0.2);
pub const MAGENTA: Color = Color::srgb(1.0, 0.28, 0.58);
pub const GREEN: Color = Color::srgb(0.3, 0.95, 0.6);
pub const RED: Color = Color::srgb(1.0, 0.38, 0.38);
/// The player's own car, and their name wherever it is listed.
pub const OWN: Color = Color::srgb(0.95, 0.45, 0.15);

pub const RADIUS: f32 = 10.0;
pub const RADIUS_SMALL: f32 = 6.0;

pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        // Loaded right away rather than in a startup system: the first screen is spawned when the
        // initial state is entered, which comes before startup systems run.
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        let regular = include_bytes!("../../assets/fonts/Saira.ttf");
        let italic = include_bytes!("../../assets/fonts/Saira-Italic.ttf");
        let fonts = Fonts {
            regular: fonts.add(Font::from_bytes(regular.to_vec())),
            italic: fonts.add(Font::from_bytes(italic.to_vec())),
        };
        app.insert_resource(fonts);
    }
}

/// The game's fonts, embedded in the executable so no asset path can go missing.
#[derive(Resource, Clone)]
pub struct Fonts {
    regular: Handle<Font>,
    italic: Handle<Font>,
}

impl Fonts {
    /// Running text and labels.
    pub fn text(&self, size: f32, weight: u16) -> TextFont {
        TextFont {
            font: self.regular.clone().into(),
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            ..default()
        }
    }

    /// Titles, big numbers and anything that should feel fast: condensed, heavy, italic.
    pub fn display(&self, size: f32) -> TextFont {
        TextFont {
            font: self.italic.clone().into(),
            font_size: FontSize::Px(size),
            weight: FontWeight(800),
            width: FontWidth(0.8),
            ..default()
        }
    }

    /// Small uppercase captions above values, such as column headers. Goes with
    /// [`CAPTION_SPACING`].
    pub fn caption(&self, size: f32) -> TextFont {
        TextFont {
            width: FontWidth(0.9),
            ..self.text(size, 600)
        }
    }
}

pub const CAPTION_SPACING: LetterSpacing = LetterSpacing::Px(1.2);

/// Behind text drawn over the 3D scene, so it reads on the road and on the neon alike.
pub const TEXT_SHADOW: TextShadow = TextShadow {
    offset: Vec2::new(0.0, 2.0),
    color: Color::srgba(0.0, 0.0, 0.0, 0.85),
};
