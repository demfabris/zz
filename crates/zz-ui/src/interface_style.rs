//! The interface styles: corner shape, outlines, shadows, row spacing and
//! floating surfaces chosen together as one [`Look`], so the chrome has a few
//! looks instead of a pile of knobs.

use serde::{Deserialize, Serialize};
use zz_gpui::{GlassMaterial, SharedString, px};

pub use zz_client::chrome_palette::InterfaceStyle;

use crate::{SelectionStyle, Theme};

/// Every theme value a style decides, in logical pixels where it is a
/// length. The storybook's style creator edits one and exports it as JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Look {
    pub radius: f32,
    pub corner_smoothing: f32,
    pub shadow: f32,
    pub elevation: f32,
    pub outline: f32,
    pub outline_width: f32,
    pub row_inset: f32,
    pub selection: SelectionStyle,
    pub density: f32,
    pub control_fill: f32,
    pub divider: f32,
    pub animation_speed: f32,
    /// The interface font family; `None` keeps the platform's.
    pub font: Option<SharedString>,
    /// What floating surfaces become; `None` fills them plainly.
    pub glass: Option<GlassMaterial>,
}

impl Default for Look {
    fn default() -> Self {
        look(InterfaceStyle::DEFAULT)
    }
}

impl Look {
    pub fn apply(&self, theme: &mut Theme) {
        theme.radius = px(self.radius);
        theme.corner_smoothing = self.corner_smoothing;
        theme.shadow = self.shadow > 0.0;
        theme.shadow_strength = self.shadow;
        theme.elevation = self.elevation;
        theme.outline = self.outline;
        theme.outline_width = px(self.outline_width);
        theme.row_inset = px(self.row_inset);
        theme.selection = self.selection;
        theme.density = self.density;
        theme.control_fill = self.control_fill;
        theme.divider = self.divider;
        theme.animation_speed = self.animation_speed;
        if let Some(font) = &self.font {
            theme.font_family = font.clone();
        }
        theme.glass = self.glass;
    }
}

/// The look `style` names.
pub fn look(style: InterfaceStyle) -> Look {
    let base = Look {
        radius: style.widget_corner_radius(),
        corner_smoothing: 4.0,
        shadow: style.shadow_strength(),
        elevation: 1.0,
        outline: 1.0,
        outline_width: 0.5,
        row_inset: 4.0,
        selection: SelectionStyle::Accent,
        density: 1.0,
        control_fill: 1.0,
        divider: 1.0,
        animation_speed: 1.0,
        font: None,
        glass: None,
    };
    let material = GlassMaterial::regular();
    match style {
        InterfaceStyle::Flat => Look {
            outline: 0.0,
            row_inset: 0.0,
            ..base.clone()
        },
        InterfaceStyle::Modern => Look {
            glass: Some(
                material
                    .blur(px(0.))
                    .tint(material.tint.alpha(1.))
                    .refraction(px(0.))
                    .bezel(px(2.))
                    .dispersion(0.)
                    .saturation(0.5)
                    .brightness(0.)
                    .specular(0.25)
                    .fresnel(0.)
                    .edge_shadow(0.3)
                    .noise(0.),
            ),
            ..base.clone()
        },
        InterfaceStyle::Full => Look {
            glass: Some(
                material
                    .blur(px(19.7))
                    .tint(material.tint.alpha(0.53))
                    .refraction(px(40.))
                    .bezel(px(11.))
                    .dispersion(0.06)
                    .saturation(0.9)
                    .brightness(0.04)
                    .specular(0.3)
                    .fresnel(0.03)
                    .edge_shadow(0.45)
                    .noise(0.),
            ),
            ..base
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_round_trip_through_json() {
        let round_trip = |look: &Look| {
            serde_json::from_str::<Look>(&serde_json::to_string(look).unwrap()).unwrap()
        };
        for style in InterfaceStyle::ALL {
            let look = look(style);
            let once = round_trip(&look);
            assert_eq!(round_trip(&once), once);
            assert_eq!(
                Look {
                    glass: None,
                    ..once.clone()
                },
                Look {
                    glass: None,
                    ..look.clone()
                }
            );
            if let (Some(before), Some(after)) = (look.glass, once.glass) {
                assert!((before.tint.a - after.tint.a).abs() <= 1. / 255.);
            }
        }
        let partial = serde_json::from_str::<Look>(r#"{"outline":0.4}"#).unwrap();
        assert_eq!(
            partial,
            Look {
                outline: 0.4,
                ..Look::default()
            }
        );
    }
}
