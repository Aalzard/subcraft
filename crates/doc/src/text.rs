//! Editable text decals. Like a pen path, the text parameters are the source of truth and the
//! paint channels of the layer are their rasterized cache at the texture set's resolution.
//! Everything is expressed in resolution-independent units (fractions of the texture width), so
//! changing the resolution or editing the text just re-rasterizes.

use std::collections::BTreeMap;

use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_text::{FontRef, TextStyle};

#[derive(Clone, Debug, PartialEq)]
pub struct TextDecal {
    pub style: TextStyle,
    pub font: FontRef,
    /// Centre of the text's layout box in texture UV space.
    pub center: Vec2,
    /// Font size (1 em) as a fraction of the texture width.
    pub size: f32,
    /// Clockwise rotation in the UV plane (UV y points down), radians.
    pub rotation: f32,
    pub opacity: f32,
    /// Linear colour / encoded data-channel values. Channels not listed stay transparent.
    pub values: BTreeMap<ChannelKind, ChannelValue>,
}

impl TextDecal {
    pub const MIN_SIZE: f32 = 0.002;
    pub const MAX_SIZE: f32 = 2.0;

    pub fn new(text: impl Into<String>, center: Vec2, values: BTreeMap<ChannelKind, ChannelValue>) -> Self {
        Self { style: TextStyle::new(text), font: FontRef::default(), center, size: 0.1, rotation: 0.0, opacity: 1.0, values }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        self.style.validate().map_err(|_| "invalid text, letter spacing or line spacing")?;
        if !self.center.is_finite() || self.center.abs().max_element() > 4.0 { return Err("text position must be within four UV tiles"); }
        if !self.size.is_finite() || !(Self::MIN_SIZE..=Self::MAX_SIZE).contains(&self.size) { return Err("text size out of range"); }
        if !self.rotation.is_finite() || self.rotation.abs() > 64.0 { return Err("invalid text rotation"); }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) { return Err("text opacity must be within 0..1"); }
        if self.values.is_empty() || self.values.values().any(|v| !v.0.iter().all(|x| x.is_finite() && x.abs() <= 65504.0)) {
            return Err("text channels must contain finite, representable values");
        }
        Ok(())
    }

    /// The embedded font file this decal needs, if any.
    pub fn resource(&self) -> Option<subcraft_core::ResourceId> {
        match &self.font { FontRef::Embedded { resource } => Some(*resource), _ => None }
    }
}
