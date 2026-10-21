use eframe::egui::{self, Color32, FontFamily, FontId, TextFormat};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use whostty_core::{alacritty_terminal::term::cell::Flags, config::Settings};

const NAMES: [&str; 4] = [
    "terminal-regular",
    "terminal-bold",
    "terminal-italic",
    "terminal-bold-italic",
];

#[derive(Clone)]
pub struct Typography {
    native: [bool; 4],
    families: [FontFamily; 4],
    pub shaping: Option<Arc<Mutex<crate::shaping::Renderer>>>,
    pub ligatures: bool,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            native: [true, false, false, false],
            shaping: None,
            ligatures: true,
            families: std::array::from_fn(|index| {
                if index == 0 {
                    FontFamily::Monospace
                } else {
                    FontFamily::Name(NAMES[index].into())
                }
            }),
        }
    }
}

impl Typography {
    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data_mut(|data| data.get_temp(egui::Id::new("terminal-typography")))
            .unwrap_or_default()
    }

    pub fn set_ligatures(ctx: &egui::Context, enabled: bool) {
        let mut typography = Self::get(ctx);
        typography.ligatures = enabled;
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("terminal-typography"), typography));
    }

    pub fn format(&self, flags: Flags, size: f32, color: Color32) -> (TextFormat, bool) {
        let style = usize::from(flags.contains(Flags::BOLD))
            + 2 * usize::from(flags.contains(Flags::ITALIC));
        let native = self.native[style];
        (
            TextFormat {
                font_id: FontId::new(size, self.families[style].clone()),
                color,
                italics: style >= 2 && !native,
                ..Default::default()
            },
            style & 1 != 0 && !native,
        )
    }
}

fn select_face(bytes: &[u8], style: usize, family: Option<&str>) -> Option<u32> {
    (0..ttf_parser::fonts_in_collection(bytes).unwrap_or(1)).find(|index| {
        let Ok(face) = ttf_parser::Face::parse(bytes, *index) else {
            return false;
        };
        face.is_monospaced()
            && face.is_bold() == (style & 1 != 0)
            && (face.is_italic() || face.is_oblique()) == (style >= 2)
            && family.is_none_or(|family| family_name(&face).as_deref() == Some(family))
    })
}

fn family_name(face: &ttf_parser::Face<'_>) -> Option<String> {
    for id in [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
    ] {
        if let Some(name) = face
            .names()
            .into_iter()
            .filter(|name| name.name_id == id)
            .find_map(|name| name.to_string())
        {
            return Some(name);
        }
    }
    None
}

fn font_data(bytes: &[u8], index: u32) -> egui::FontData {
    let (bytes, index) = crate::font_collection::extract(bytes, index)
        .map_or_else(|| (bytes.to_vec(), index), |bytes| (bytes, 0));
    egui::FontData {
        index,
        ..egui::FontData::from_owned(bytes)
    }
}

fn read_font(path: &Path) -> std::io::Result<Vec<u8>> {
    const LIMIT: u64 = 64 * 1024 * 1024;
    let file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > LIMIT {
        return Err(std::io::Error::other("Font files must be at most 64 MiB"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(std::io::Error::other("Font files must be at most 64 MiB"));
    }
    Ok(bytes.into_boxed_slice().into_vec())
}

pub fn configure_fonts(ctx: &egui::Context, settings: &Settings) -> Option<String> {
    let mut fonts = egui::FontDefinitions::default();
    let mut typography = Typography::default();
    let mut errors = Vec::new();
    let mut paths: Vec<PathBuf> = settings.font_path.clone().into_iter().collect();
    #[cfg(windows)]
    let root = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
        .join("Fonts");
    #[cfg(windows)]
    paths.extend([root.join("CascadiaMono.ttf"), root.join("consola.ttf")]);
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from("/System/Library/Fonts/Menlo.ttc"));
    let mut selected = None;
    for path in paths {
        match read_font(&path) {
            Ok(bytes) => {
                let Some(index) = select_face(&bytes, 0, None) else {
                    if settings.font_path.as_deref() == Some(&path) {
                        errors.push(format!("No regular monospace face in {}", path.display()));
                    }
                    continue;
                };
                let family = ttf_parser::Face::parse(&bytes, index)
                    .ok()
                    .and_then(|face| family_name(&face));
                fonts
                    .font_data
                    .insert(NAMES[0].into(), font_data(&bytes, index).into());
                fonts
                    .families
                    .get_mut(&FontFamily::Monospace)
                    .unwrap()
                    .insert(0, NAMES[0].into());
                selected = Some((path, bytes, family));
                break;
            }
            Err(error) if settings.font_path.as_deref() == Some(&path) => {
                errors.push(format!("Load font {}: {error}", path.display()))
            }
            Err(_) => {}
        }
    }
    let custom = [
        None,
        settings.font_bold_path.as_deref(),
        settings.font_italic_path.as_deref(),
        settings.font_bold_italic_path.as_deref(),
    ];
    for style in 1..4 {
        let family = typography.families[style].clone();
        let mut fallbacks = fonts.families[&FontFamily::Monospace].clone();
        let expected_family = selected
            .as_ref()
            .and_then(|(_, _, family)| family.as_deref());
        let mut face = None;
        if let Some(path) = custom[style] {
            match read_font(path) {
                Ok(bytes) => {
                    if let Some(index) = select_face(&bytes, style, expected_family) {
                        face = Some((bytes, index));
                    } else {
                        errors.push(format!("No matching monospace style in {}", path.display()));
                    }
                }
                Err(error) => errors.push(format!("Load font {}: {error}", path.display())),
            }
        }
        if face.is_none() {
            if let Some((_, bytes, family)) = &selected {
                if let Some(index) = select_face(bytes, style, family.as_deref()) {
                    face = Some((bytes.clone(), index));
                }
            }
        }
        #[cfg(windows)]
        if face.is_none() && settings.font_path.is_none() {
            for name in match style {
                1 => ["consolab.ttf", "CascadiaMono-Bold.ttf"],
                2 => ["consolai.ttf", "CascadiaMono-Italic.ttf"],
                _ => ["consolaz.ttf", "CascadiaMono-BoldItalic.ttf"],
            } {
                if let Ok(bytes) = read_font(&root.join(name)) {
                    if let Some(index) = select_face(&bytes, style, expected_family) {
                        face = Some((bytes, index));
                        break;
                    }
                }
            }
        }
        if let Some((bytes, index)) = face {
            fonts
                .font_data
                .insert(NAMES[style].into(), font_data(&bytes, index).into());
            fallbacks.insert(0, NAMES[style].into());
            typography.native[style] = true;
        }
        fonts.families.insert(family, fallbacks);
    }
    #[cfg(windows)]
    let cjk = vec![root.join("malgun.ttf")];
    #[cfg(target_os = "macos")]
    let cjk = vec![PathBuf::from("/System/Library/Fonts/AppleSDGothicNeo.ttc")];
    #[cfg(not(any(windows, target_os = "macos")))]
    let cjk: Vec<PathBuf> = Vec::new();
    for path in cjk {
        if let Ok(bytes) = read_font(&path) {
            if ttf_parser::Face::parse(&bytes, 0).is_ok() {
                fonts
                    .font_data
                    .insert("terminal-cjk".into(), font_data(&bytes, 0).into());
                for family in fonts.families.values_mut() {
                    family.push("terminal-cjk".into());
                }
            }
        }
    }
    let sources = std::array::from_fn(|style| {
        fonts.families[&typography.families[style]]
            .iter()
            .filter_map(|name| fonts.font_data.get(name).cloned())
            .collect()
    });
    typography.shaping = Some(Arc::new(Mutex::new(crate::shaping::Renderer::new(sources))));
    typography.ligatures = settings.font_ligatures;
    ctx.set_fonts(fonts);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("terminal-typography"), typography));
    (!errors.is_empty()).then(|| errors.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_custom_font_falls_back_and_all_style_families_remain_renderable() {
        let ctx = egui::Context::default();
        let settings = Settings {
            font_path: Some(std::env::temp_dir().join("whostty-font-does-not-exist.ttf")),
            ..Settings::default()
        };
        assert!(configure_fonts(&ctx, &settings)
            .unwrap()
            .contains("Load font"));
        let _ = ctx.run(Default::default(), |ctx| {
            let typography = Typography::get(ctx);
            for flags in [
                Flags::empty(),
                Flags::BOLD,
                Flags::ITALIC,
                Flags::BOLD | Flags::ITALIC,
            ] {
                let (format, _) = typography.format(flags, 15.0, Color32::WHITE);
                let width = ctx.fonts(|fonts| fonts.glyph_width(&format.font_id, 'M'));
                assert!(width.is_finite() && width > 0.0);
            }
        });
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn collection_faces_preserve_regular_bold_italic_and_combined_styles() {
        let bytes = std::fs::read("/System/Library/Fonts/Menlo.ttc").unwrap();
        let mut indices = std::collections::BTreeSet::new();
        for style in 0..4 {
            let index = select_face(&bytes, style, None).expect("Missing Menlo style");
            indices.insert(index);
            let data = font_data(&bytes, index);
            assert!(data.font.len() < bytes.len() / 2);
            assert_eq!(select_face(&data.font, style, None), Some(0));
        }
        assert_eq!(indices.len(), 4);
    }
}
