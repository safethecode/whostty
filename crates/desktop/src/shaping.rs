use ab_glyph::{Font, ScaleFont};
use eframe::egui::{
    self, Color32, ColorImage, FontData, Mesh, Painter, Pos2, Rect, TextureHandle, Vec2,
};
use rustybuzz::{Direction, Feature, UnicodeBuffer};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use unicode_script::{Script, UnicodeScript};

const ATLAS_SIDE: usize = 2048;
const MAX_LAYOUTS: usize = 512;
const MAX_LAYOUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_RUN_BYTES: usize = 8192;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Stop {
    pub byte: usize,
    pub column: usize,
}

#[derive(Default)]
pub struct Run {
    pub text: String,
    pub stops: Vec<Stop>,
    pub columns: usize,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct LayoutKey {
    text: String,
    stops: Vec<Stop>,
    columns: usize,
    style: usize,
    size: u32,
    density: u32,
    cell_width: u32,
    ligatures: bool,
}

#[derive(Clone)]
struct Positioned {
    source: usize,
    id: u16,
    size: u16,
    position: Vec2,
    baseline: f32,
    bold: bool,
    italic: bool,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct GlyphKey {
    source: usize,
    id: u16,
    size: u16,
}

#[derive(Clone, Copy)]
struct CachedGlyph {
    uv: Rect,
    offset: Vec2,
    size: Vec2,
}

struct Atlas {
    texture: Option<TextureHandle>,
    glyphs: HashMap<GlyphKey, Option<CachedGlyph>>,
    x: usize,
    y: usize,
    row: usize,
    full: bool,
    frame: u64,
}

impl Default for Atlas {
    fn default() -> Self {
        Self {
            texture: None,
            glyphs: HashMap::new(),
            x: 1,
            y: 1,
            row: 0,
            full: false,
            frame: u64::MAX,
        }
    }
}

impl Atlas {
    fn begin(&mut self, ctx: &egui::Context) {
        let frame = ctx.cumulative_pass_nr();
        // Never overwrite UVs already referenced by this frame, including other panes.
        if self.full && self.frame != frame {
            self.glyphs.clear();
            self.x = 1;
            self.y = 1;
            self.row = 0;
            self.full = false;
            if let Some(texture) = &mut self.texture {
                texture.set(
                    ColorImage::new([ATLAS_SIDE; 2], Color32::TRANSPARENT),
                    egui::TextureOptions::LINEAR,
                );
            }
        }
        self.frame = frame;
    }

    fn glyph(
        &mut self,
        ctx: &egui::Context,
        sources: &[Arc<FontData>],
        key: GlyphKey,
    ) -> Option<Option<CachedGlyph>> {
        if let Some(glyph) = self.glyphs.get(&key) {
            return Some(*glyph);
        }
        if self.glyphs.len() >= 8192 {
            self.full = true;
            return None;
        }
        let data = &sources[key.source];
        // A color/SVG/bitmap glyph can also contain a monochrome outline. Keep the
        // complete glyph on the existing fallback path until those formats are supported.
        let face = ttf_parser::Face::parse(&data.font, data.index).ok()?;
        let id = ttf_parser::GlyphId(key.id);
        if face.is_color_glyph(id)
            || face.glyph_svg_image(id).is_some()
            || face.glyph_raster_image(id, key.size).is_some()
        {
            return None;
        }
        let font = ab_glyph::FontRef::try_from_slice_and_index(&data.font, data.index).ok()?;
        let outline = font.outline_glyph(ab_glyph::GlyphId(key.id).with_scale(key.size as f32));
        let Some(outline) = outline else {
            // OpenType contextual ligatures can emit blank, advancing placeholder glyphs.
            // Absence of outlines AND bitmap/color/SVG data means intentional empty ink.
            self.glyphs.insert(key, None);
            return Some(None);
        };
        let bounds = outline.px_bounds();
        let width = bounds.width() as usize;
        let height = bounds.height() as usize;
        if width == 0 || height == 0 {
            self.glyphs.insert(key, None);
            return Some(None);
        }
        if width + 2 > ATLAS_SIDE || height + 2 > ATLAS_SIDE {
            return None;
        }
        if self.x + width + 1 > ATLAS_SIDE {
            self.x = 1;
            self.y += self.row + 1;
            self.row = 0;
        }
        if self.y + height + 1 > ATLAS_SIDE {
            self.full = true;
            return None;
        }
        let mut image = ColorImage::new([width, height], Color32::TRANSPARENT);
        outline.draw(|x, y, coverage| {
            // Match egui's coverage gamma so custom glyphs retain the existing text weight.
            let alpha = (coverage.powf(0.55) * 255.0).round() as u8;
            image[(x as usize, y as usize)] =
                Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha);
        });
        let texture = self.texture.get_or_insert_with(|| {
            ctx.load_texture(
                "terminal-shaped-glyphs",
                ColorImage::new([ATLAS_SIDE; 2], Color32::TRANSPARENT),
                egui::TextureOptions::LINEAR,
            )
        });
        texture.set_partial([self.x, self.y], image, egui::TextureOptions::LINEAR);
        let glyph = CachedGlyph {
            uv: Rect::from_min_max(
                Pos2::new(
                    self.x as f32 / ATLAS_SIDE as f32,
                    self.y as f32 / ATLAS_SIDE as f32,
                ),
                Pos2::new(
                    (self.x + width) as f32 / ATLAS_SIDE as f32,
                    (self.y + height) as f32 / ATLAS_SIDE as f32,
                ),
            ),
            offset: Vec2::new(bounds.min.x, bounds.min.y),
            size: Vec2::new(width as f32, height as f32),
        };
        self.x += width + 1;
        self.row = self.row.max(height);
        self.glyphs.insert(key, Some(glyph));
        Some(Some(glyph))
    }
}

struct CachedLayout {
    glyphs: Option<Arc<Vec<Positioned>>>,
    bytes: usize,
}

pub struct Renderer {
    sources: Vec<Arc<FontData>>,
    families: [Vec<usize>; 4],
    layouts: HashMap<LayoutKey, CachedLayout>,
    layout_bytes: usize,
    drawn_runs: u64,
    order: VecDeque<LayoutKey>,
    atlas: Atlas,
}

pub struct Paint<'a> {
    pub run: &'a Run,
    pub pos: Pos2,
    pub size: f32,
    pub cell_width: f32,
    pub style: usize,
    pub color: Color32,
    pub ligatures: bool,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
}

impl Renderer {
    pub fn new(families: [Vec<Arc<FontData>>; 4]) -> Self {
        let mut sources: Vec<Arc<FontData>> = Vec::new();
        let families = families.map(|family| {
            family
                .into_iter()
                .map(|data| {
                    if let Some(index) = sources
                        .iter()
                        .position(|existing| Arc::ptr_eq(existing, &data))
                    {
                        index
                    } else {
                        let index = sources.len();
                        sources.push(data);
                        index
                    }
                })
                .collect()
        });
        Self {
            sources,
            families,
            layouts: HashMap::new(),
            layout_bytes: 0,
            drawn_runs: 0,
            order: VecDeque::new(),
            atlas: Default::default(),
        }
    }

    pub fn diagnostics(&self) -> (usize, usize, u64) {
        (self.atlas.glyphs.len(), self.layouts.len(), self.drawn_runs)
    }

    pub fn memory_report(&self) -> serde_json::Value {
        let bytes: usize = self.sources.iter().map(|data| data.font.len()).sum();
        let capacity: usize = self
            .sources
            .iter()
            .map(|data| match &data.font {
                std::borrow::Cow::Owned(bytes) => bytes.capacity(),
                std::borrow::Cow::Borrowed(bytes) => bytes.len(),
            })
            .sum();
        serde_json::json!({
            "unique_font_sources": self.sources.len(),
            "font_source_bytes": bytes,
            "font_source_capacity_bytes": capacity,
            "layout_cache_estimated_bytes": self.layout_bytes,
            "glyph_entries": self.atlas.glyphs.len(),
            "atlas_rgba_bytes": if self.atlas.texture.is_some() { ATLAS_SIDE * ATLAS_SIDE * 4 } else { 0 },
        })
    }

    pub fn paint(&mut self, painter: &Painter, paint: Paint<'_>) -> bool {
        let density = painter.ctx().pixels_per_point();
        self.atlas.begin(painter.ctx());
        if paint.run.text.len() > MAX_RUN_BYTES
            || paint.run.text.is_empty()
            || self.families[paint.style].is_empty()
        {
            return false;
        }
        let key = LayoutKey {
            text: paint.run.text.clone(),
            stops: paint.run.stops.clone(),
            columns: paint.run.columns,
            style: paint.style,
            size: paint.size.to_bits(),
            density: density.to_bits(),
            cell_width: paint.cell_width.to_bits(),
            ligatures: paint.ligatures,
        };
        let layout = if let Some(layout) = self.layouts.get(&key) {
            layout.glyphs.clone()
        } else {
            // Cache unsupported runs too: repeated RTL/bitmap fallback must stay cheap.
            let layout = self.shape(&key).map(Arc::new);
            let bytes = key.text.len() * 2
                + key.stops.len() * std::mem::size_of::<Stop>() * 2
                + layout
                    .as_ref()
                    .map_or(0, |glyphs| glyphs.len() * std::mem::size_of::<Positioned>());
            while self.layouts.len() >= MAX_LAYOUTS || self.layout_bytes + bytes > MAX_LAYOUT_BYTES
            {
                let Some(oldest) = self.order.pop_front() else {
                    break;
                };
                if let Some(old) = self.layouts.remove(&oldest) {
                    self.layout_bytes -= old.bytes;
                }
            }
            if bytes <= MAX_LAYOUT_BYTES {
                self.layout_bytes += bytes;
                self.layouts.insert(
                    key.clone(),
                    CachedLayout {
                        glyphs: layout.clone(),
                        bytes,
                    },
                );
                self.order.push_back(key);
            }
            layout
        };
        let Some(layout) = layout else {
            return false;
        };
        let mut quads = Vec::with_capacity(layout.len());
        for glyph in layout.iter() {
            let Some(cached) = self.atlas.glyph(
                painter.ctx(),
                &self.sources,
                GlyphKey {
                    source: glyph.source,
                    id: glyph.id,
                    size: glyph.size,
                },
            ) else {
                return false;
            };
            if let Some(cached) = cached {
                quads.push((
                    glyph.position,
                    cached,
                    glyph.bold,
                    glyph.italic,
                    glyph.baseline,
                ));
            }
        }
        let Some(texture) = &self.atlas.texture else {
            return true;
        };
        let mut mesh = Mesh::with_texture(texture.id());
        for (position, glyph, bold, italic, baseline) in quads {
            let origin = paint.pos + (position + glyph.offset) / density;
            let origin = Pos2::new(
                (origin.x * density).round() / density,
                (origin.y * density).round() / density,
            );
            let rect = Rect::from_min_size(origin, glyph.size / density);
            let copies = if paint.synthetic_bold || bold { 2 } else { 1 };
            for copy in 0..copies {
                let rect =
                    rect.translate(Vec2::new(copy as f32 * (paint.size * 0.025).max(0.25), 0.0));
                let start = mesh.vertices.len();
                mesh.add_rect_with_uv(rect, glyph.uv, paint.color);
                if paint.synthetic_italic || italic {
                    for vertex in &mut mesh.vertices[start..] {
                        vertex.pos.x += (paint.pos.y + baseline / density - vertex.pos.y) * 0.2;
                    }
                }
            }
        }
        if !mesh.vertices.is_empty() {
            self.drawn_runs = self.drawn_runs.saturating_add(1);
            painter.add(mesh);
        }
        true
    }

    fn shape(&self, key: &LayoutKey) -> Option<Vec<Positioned>> {
        let density = f32::from_bits(key.density);
        let size = f32::from_bits(key.size) * density;
        let cell = f32::from_bits(key.cell_width) * density;
        let mut result = Vec::new();
        let stops = &key.stops;
        let mut start = 0;
        while start < stops.len() {
            let cell_end = |index: usize| {
                stops
                    .get(index + 1)
                    .map_or(key.text.len(), |stop| stop.byte)
            };
            let source_for = |index: usize| {
                self.families[key.style].iter().copied().find(|source| {
                    let data = &self.sources[*source];
                    let Some(face) = rustybuzz::Face::from_slice(&data.font, data.index) else {
                        return false;
                    };
                    key.text[stops[index].byte..cell_end(index)]
                        .chars()
                        .all(|c| {
                            matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')
                                || face.glyph_index(c).is_some()
                        })
                })
            };
            let source = source_for(start)?;
            let script_for = |index: usize| {
                key.text[stops[index].byte..cell_end(index)]
                    .chars()
                    .map(|c| c.script())
                    .find(|script| !matches!(script, Script::Common | Script::Inherited))
            };
            let mut script = script_for(start);
            let mut end = start + 1;
            while end < stops.len() && source_for(end) == Some(source) {
                let next = script_for(end);
                if script.is_some() && next.is_some() && next != script {
                    break;
                }
                if script.is_none() {
                    script = next;
                }
                end += 1;
            }
            let byte_start = stops[start].byte;
            let byte_end = stops.get(end).map_or(key.text.len(), |stop| stop.byte);
            let data = &self.sources[source];
            let face = rustybuzz::Face::from_slice(&data.font, data.index)?;
            let font = ab_glyph::FontRef::try_from_slice_and_index(&data.font, data.index).ok()?;
            // egui interprets font size in em units before converting to ab_glyph's
            // ascent-minus-descent scale. Use the same conversion for cell agreement.
            let base_scale = size * font.height_unscaled() / font.units_per_em()?;
            let visual_scale = base_scale * data.tweak.scale;
            let pixels = visual_scale.round();
            if !(1.0..=512.0).contains(&pixels) {
                return None;
            }
            let pixels = pixels as u16;
            let factor = font.as_scaled(pixels as f32).h_scale_factor();
            let metrics = font.as_scaled(base_scale);
            let baseline = metrics.ascent()
                + visual_scale * (data.tweak.y_offset_factor + data.tweak.baseline_offset_factor)
                + data.tweak.y_offset * density
                - (1.0 - data.tweak.scale) * 0.5 * (metrics.ascent() + metrics.descent());
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(&key.text[byte_start..byte_end]);
            buffer.guess_segment_properties();
            // Cell order stays owned by the terminal. RTL/bidi needs a separate policy.
            if buffer.direction() != Direction::LeftToRight {
                return None;
            }
            let mut features = vec![Feature::new(
                rustybuzz::ttf_parser::Tag::from_bytes(b"kern"),
                0,
                ..,
            )];
            if !key.ligatures {
                for tag in [b"liga", b"clig", b"calt", b"dlig", b"hlig"] {
                    features.push(Feature::new(
                        rustybuzz::ttf_parser::Tag::from_bytes(tag),
                        0,
                        ..,
                    ));
                }
            }
            let shaped = rustybuzz::shape(&face, &features, buffer);
            let infos = shaped.glyph_infos();
            let positions = shaped.glyph_positions();
            let column_at = |byte: usize| {
                if byte >= key.text.len() {
                    key.columns
                } else {
                    stops
                        .iter()
                        .rev()
                        .find(|stop| stop.byte <= byte)
                        .map_or(0, |stop| stop.column)
                }
            };
            let mut index = 0;
            while index < infos.len() {
                let cluster = infos[index].cluster;
                let mut next = index + 1;
                while next < infos.len() && infos[next].cluster == cluster {
                    next += 1;
                }
                if infos.get(next).is_some_and(|info| info.cluster < cluster) {
                    return None;
                }
                let next_byte = infos
                    .get(next)
                    .map_or(byte_end, |info| byte_start + info.cluster as usize);
                let column = column_at(byte_start + cluster as usize);
                let available = column_at(next_byte).saturating_sub(column) as f32 * cell;
                let advance = positions[index..next]
                    .iter()
                    .map(|pos| pos.x_advance as f32)
                    .sum::<f32>()
                    * factor;
                let mut pen = column as f32 * cell + (available - advance) * 0.5;
                for (info, pos) in infos[index..next].iter().zip(&positions[index..next]) {
                    result.push(Positioned {
                        source,
                        baseline,
                        id: info.glyph_id.try_into().ok()?,
                        size: pixels,
                        bold: key.style & 1 != 0 && !face.is_bold(),
                        italic: key.style >= 2 && !(face.is_italic() || face.is_oblique()),
                        position: Vec2::new(
                            pen + pos.x_offset as f32 * factor,
                            baseline - pos.y_offset as f32 * factor,
                        ),
                    });
                    pen += pos.x_advance as f32 * factor;
                }
                index = next;
            }
            start = end;
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Renderer {
        let data = Arc::new(FontData::from_static(include_bytes!(
            "../tests/fixtures/shaping.ttf"
        )));
        Renderer::new(std::array::from_fn(|_| vec![data.clone()]))
    }

    fn key(text: &str, stops: &[usize], columns: usize, ligatures: bool) -> LayoutKey {
        LayoutKey {
            text: text.into(),
            stops: stops
                .iter()
                .enumerate()
                .map(|(column, byte)| Stop {
                    byte: *byte,
                    column,
                })
                .collect(),
            columns,
            style: 0,
            size: 20.0f32.to_bits(),
            density: 1.0f32.to_bits(),
            cell_width: 12.0f32.to_bits(),
            ligatures,
        }
    }

    #[test]
    #[ignore = "Explicit release CPU benchmark; not a GPU or input latency measurement"]
    fn render_submission_benchmark() {
        use std::time::Instant;
        let ctx = egui::Context::default();
        let settings = whostty_core::config::Settings::default();
        let _ = crate::fonts::configure_fonts(&ctx, &settings);
        let renderer = crate::fonts::Typography::get(&ctx).shaping.unwrap();
        let rows: Vec<_> = (0..40)
            .map(|row| {
                let text = format!(
                    "row={row:02} const result = input.map(value => value + 1); // terminal text "
                );
                let text: String = text
                    .chars()
                    .chain(std::iter::repeat(' '))
                    .take(80)
                    .collect();
                Run {
                    stops: text
                        .char_indices()
                        .enumerate()
                        .map(|(column, (byte, _))| Stop { byte, column })
                        .collect(),
                    text,
                    columns: 80,
                }
            })
            .collect();
        let draw = || {
            let started = Instant::now();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 1100.0))),
                    ..Default::default()
                },
                |ctx| {
                    let cell_width =
                        ctx.fonts(|fonts| fonts.glyph_width(&egui::FontId::monospace(17.0), 'M'));
                    let painter = ctx.layer_painter(egui::LayerId::background());
                    let mut renderer = renderer.lock().unwrap();
                    for (row, run) in rows.iter().enumerate() {
                        assert!(renderer.paint(
                            &painter,
                            Paint {
                                run,
                                pos: Pos2::new(0.0, row as f32 * 24.0),
                                size: 17.0,
                                cell_width,
                                style: 0,
                                color: Color32::WHITE,
                                ligatures: true,
                                synthetic_bold: false,
                                synthetic_italic: false
                            }
                        ));
                    }
                },
            );
            let _ = ctx.tessellate(output.shapes, ctx.pixels_per_point());
            started.elapsed().as_secs_f64() * 1000.0
        };
        let cold_ms = draw();
        for _ in 0..10 {
            draw();
        }
        let mut samples: Vec<_> = (0..200).map(|_| draw()).collect();
        samples.sort_by(f64::total_cmp);
        let (glyphs, layouts, _) = renderer.lock().unwrap().diagnostics();
        println!(
            "{}",
            serde_json::json!({"case":"shaped_render_cpu_submission","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"release":!cfg!(debug_assertions),"columns":80,"rows":40,"warm_frames":200,"cold_ms":cold_ms,"warm_p50_ms":samples[100],"warm_p95_ms":samples[190],"glyphs_cached":glyphs,"layouts_cached":layouts,"gpu_submission_included":false,"input_latency_included":false})
        );
    }

    #[test]
    fn opentype_ligatures_and_marks_keep_the_terminal_cell_extent() {
        let renderer = renderer();
        let ligature = renderer.shape(&key("fi", &[0, 1], 2, true)).unwrap();
        let separate = renderer.shape(&key("fi", &[0, 1], 2, false)).unwrap();
        assert!(
            renderer.shape(&key("fا", &[0, 1], 2, true)).is_none(),
            "Mixed RTL text must not inherit the Latin shaping direction"
        );
        assert_eq!(ligature.len(), 1);
        assert_eq!(ligature[0].id, 5);
        assert_eq!(separate.len(), 2);
        assert!((ligature[0].position.x - separate[0].position.x).abs() < 0.001);
        assert!((separate[1].position.x - separate[0].position.x - 12.0).abs() < 0.001);
        let marks = renderer.shape(&key("e\u{301}", &[0], 1, true)).unwrap();
        assert_eq!(marks.len(), 2);
        assert!(
            marks[1].position.y < marks[0].position.y - 10.0,
            "OpenType mark anchor was ignored"
        );
        assert!((marks[1].position.x - marks[0].position.x - 6.0).abs() < 0.001);
        // A two-cell glyph is followed by a one-cell character at the original grid column.
        let mut wide_key = key("界f", &[0, 3], 3, true);
        wide_key.stops[1].column = 2;
        let positioned = renderer.shape(&wide_key).unwrap();
        assert_eq!(positioned.len(), 2);
        assert!((positioned[1].position.x - 24.0).abs() < 0.001);
    }

    #[test]
    fn cache_limits_and_pass_boundaries_keep_existing_glyph_uvs_valid() {
        let ctx = egui::Context::default();
        let mut renderer = renderer();
        let _ = ctx.run(Default::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            for index in 0..1100 {
                let run = Run {
                    text: format!("{}{}", "X".repeat(8000), index),
                    stops: vec![Stop { byte: 0, column: 0 }],
                    columns: 1,
                };
                assert!(!renderer.paint(
                    &painter,
                    Paint {
                        run: &run,
                        pos: Pos2::ZERO,
                        size: 20.0,
                        cell_width: 12.0,
                        style: 0,
                        color: Color32::WHITE,
                        ligatures: true,
                        synthetic_bold: false,
                        synthetic_italic: false
                    }
                ));
            }
            assert!(renderer.layouts.len() <= MAX_LAYOUTS);
            assert!(renderer.layout_bytes <= MAX_LAYOUT_BYTES);
            let first = GlyphKey {
                source: 0,
                id: 5,
                size: 512,
            };
            renderer.atlas.begin(ctx);
            let uv = renderer
                .atlas
                .glyph(ctx, &renderer.sources, first)
                .unwrap()
                .unwrap()
                .uv;
            for size in (400..512).rev() {
                if renderer
                    .atlas
                    .glyph(ctx, &renderer.sources, GlyphKey { size, ..first })
                    .is_none()
                {
                    break;
                }
            }
            assert!(renderer.atlas.full, "Fixture did not fill the atlas");
            renderer.atlas.begin(ctx);
            assert_eq!(
                renderer.atlas.glyphs[&first].unwrap().uv,
                uv,
                "Atlas was recycled while this pass still referenced it"
            );
        });
        let _ = ctx.run(Default::default(), |ctx| {
            renderer.atlas.begin(ctx);
            assert!(!renderer.atlas.full);
            assert!(renderer.atlas.glyphs.is_empty());
        });
    }

    #[test]
    fn glyph_ids_are_rasterized_and_cached_without_reuploading_unchanged_text() {
        let ctx = egui::Context::default();
        let mut renderer = renderer();
        // The SVG fixture also has a monochrome outline; it must not be misrepresented.
        assert!(renderer
            .atlas
            .glyph(
                &ctx,
                &renderer.sources,
                GlyphKey {
                    source: 0,
                    id: 0,
                    size: 28
                }
            )
            .is_none());
        let run = Run {
            text: "fi e\u{301}\u{2007}".into(),
            stops: vec![
                Stop { byte: 0, column: 0 },
                Stop { byte: 1, column: 1 },
                Stop { byte: 2, column: 2 },
                Stop { byte: 3, column: 3 },
                Stop { byte: 6, column: 4 },
            ],
            columns: 5,
        };
        let mut render = |ctx: &egui::Context| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            assert!(renderer.paint(
                &painter,
                Paint {
                    run: &run,
                    pos: Pos2::ZERO,
                    size: 20.0,
                    cell_width: 12.0,
                    style: 0,
                    color: Color32::GREEN,
                    ligatures: true,
                    synthetic_bold: false,
                    synthetic_italic: false
                }
            ));
        };
        let first = ctx.run(Default::default(), &mut render);
        let second = ctx.run(Default::default(), &mut render);
        assert!(first
            .textures_delta
            .set
            .iter()
            .any(|(_, delta)| delta.pos.is_some() && delta.image.width() > 0));
        assert!(
            second.textures_delta.set.is_empty(),
            "Unchanged glyphs were uploaded again"
        );
        assert_eq!(renderer.layouts.len(), 1);
        assert_eq!(renderer.atlas.glyphs.len(), 5); // plus a blank advancing ligature placeholder.
        assert!(!second.shapes.is_empty());
    }
}
