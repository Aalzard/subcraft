//! Image assignment is tool state. Only subsequent strokes modify the document/history.
use crate::{Command, Session, TextureSetImport};
use std::path::{Path, PathBuf};
use subcraft_assets::texture_maps::{self, Classified, MapConversion};
use subcraft_core::ChannelKind;

impl Session {
    /// Same file-name conversions as fill channels, with no change to the selected layer.
    pub fn set_brush_channel_image(
        &mut self,
        channel: ChannelKind,
        path: &Path,
    ) -> Result<(), String> {
        if !self.selected_texture_set().has_channel(channel) {
            return Err("texture set has no such channel".into());
        }
        let conversion = match texture_maps::classify(
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default(),
        ) {
            Classified::Map(m) if m.channel == channel => m.conversion,
            _ => MapConversion::None,
        };
        let resource = self.import_converted(path, conversion)?;
        let mut material = self.brush.material.clone();
        material.textures.insert(channel, resource);
        material
            .channels
            .entry(channel)
            .or_insert(subcraft_paint::BrushChannel {
                enabled: true,
                value: channel.default_paint_value(),
            })
            .enabled = true;
        self.execute(Command::SetBrushMaterial(material))
            .map_err(|e| e.to_string())
    }

    /// Load a named texture set into the brush. Unmatched channels are disabled, so painting a
    /// normal-only material never overwrites color/roughness with their previous flat values.
    /// Assignment is atomic on failure; decoded resources may remain in the immutable store.
    pub fn import_brush_texture_set(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<TextureSetImport, String> {
        let plan = texture_maps::plan(paths);
        let mut result = TextureSetImport {
            layer_name: plan.name,
            ..Default::default()
        };
        result.skipped = plan
            .skipped
            .iter()
            .map(|(p, why)| (p.display().to_string(), why.clone()))
            .collect();
        let mut material = self.brush.material.clone();
        material.textures.clear();
        for channel in material.channels.values_mut() {
            channel.enabled = false;
        }
        for (path, map) in plan.maps {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string();
            if !self.selected_texture_set().has_channel(map.channel) {
                result.skipped.push((
                    name,
                    format!("texture set has no {} channel", map.channel.display_name()),
                ));
                continue;
            }
            let id = self.import_converted(&path, map.conversion)?;
            material.textures.insert(map.channel, id);
            material.channels.insert(
                map.channel,
                subcraft_paint::BrushChannel {
                    enabled: true,
                    value: map.channel.default_paint_value(),
                },
            );
            result.used.push((name, map.channel));
        }
        if result.used.is_empty() {
            return Err("no recognized maps match this texture set's channels".into());
        }
        self.execute(Command::SetBrushMaterial(material))
            .map_err(|e| e.to_string())?;
        Ok(result)
    }
}
