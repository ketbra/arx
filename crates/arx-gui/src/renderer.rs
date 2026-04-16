//! wgpu + glyphon renderer.
//!
//! Maintains a local [`CellGrid`] mirror that receives [`DiffOp`]
//! batches from the editor thread. On each redraw the grid is
//! rasterised to the window surface:
//!
//! 1. **Background pass**: coloured rectangles via an instanced quad
//!    pipeline (one instance per cell whose bg differs from the
//!    default, plus one for the cursor).
//! 2. **Text pass**: glyphon renders every cell's grapheme with the
//!    cell's foreground colour via per-span `cosmic_text::Attrs`.
//!
//! The split keeps text legible on top of arbitrary cell backgrounds
//! without Z-fighting.

use glyphon::{
    Attrs, Buffer as GlyphonBuffer, Cache as GlyphonCache, Color as GlyphonColor, Family,
    FontSystem, Metrics, Resolution, Shaping, Style, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer, Viewport, Weight,
};
use wgpu::util::DeviceExt;

use arx_render::face::Color as ArxColor;
use arx_render::{CellGrid, CursorRender, DiffOp, ResolvedFace};

// ---------------------------------------------------------------------------
// Background-rect pipeline types
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RectInstance {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
}

impl RectInstance {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}

const RECT_SHADER: &str = r"
struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct Uniforms {
    screen: vec2<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    let ndc = vec2<f32>(
        px.x / u.screen.x * 2.0 - 1.0,
        1.0 - px.y / u.screen.y * 2.0,
    );
    var out: VsOut;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
";

// ---------------------------------------------------------------------------
// Span coalescing helper
// ---------------------------------------------------------------------------

/// Style key for run-length coalescing of text spans.
struct SpanStyle {
    fg: ArxColor,
    bold: bool,
    italic: bool,
}

impl SpanStyle {
    fn matches(&self, fg: ArxColor, bold: bool, italic: bool) -> bool {
        self.fg == fg && self.bold == bold && self.italic == italic
    }
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub struct GuiRenderer {
    // Note: wgpu + glyphon types don't implement Debug,
    // so we implement it manually below.
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // glyphon
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,

    // bg-rect pipeline
    rect_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    // editor mirror
    grid: CellGrid,
    cursor: Option<CursorRender>,

    // monospace cell metrics (pixels)
    cell_w: f32,
    cell_h: f32,
}

impl std::fmt::Debug for GuiRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuiRenderer")
            .field("grid", &format_args!("{}x{}", self.grid.width(), self.grid.height()))
            .field("cell_w", &self.cell_w)
            .field("cell_h", &self.cell_h)
            .finish_non_exhaustive()
    }
}

fn arx_to_wgpu_color(c: ArxColor) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(c.r()) / 255.0,
        g: f64::from(c.g()) / 255.0,
        b: f64::from(c.b()) / 255.0,
        a: 1.0,
    }
}

fn arx_color_to_f32(c: ArxColor) -> [f32; 4] {
    [
        f32::from(c.r()) / 255.0,
        f32::from(c.g()) / 255.0,
        f32::from(c.b()) / 255.0,
        1.0,
    ]
}

fn arx_to_glyphon_color(c: ArxColor) -> GlyphonColor {
    GlyphonColor::rgba(c.r(), c.g(), c.b(), 255)
}

impl GuiRenderer {
    #[allow(clippy::too_many_lines)]
    pub async fn new(
        window: std::sync::Arc<winit::window::Window>,
        cols: u16,
        rows: u16,
    ) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .expect("no suitable GPU adapter found");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("failed to create device");
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // ---- glyphon init ----
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let glyphon_cache = GlyphonCache::new(&device);
        let mut atlas = TextAtlas::new(&device, &queue, &glyphon_cache, surface_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let mut viewport = Viewport::new(&device, &glyphon_cache);
        viewport.update(
            &queue,
            Resolution {
                width: surface_config.width,
                height: surface_config.height,
            },
        );

        let font_size = 16.0_f32;
        let line_height = (font_size * 1.4).ceil();
        let cell_w = Self::probe_cell_width(&mut font_system, font_size);
        let cell_h = line_height;

        // ---- rect pipeline ----
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rect_uniforms"),
            contents: bytemuck::cast_slice(&[size.width as f32, size.height as f32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect_bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[RectInstance::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            device,
            queue,
            surface,
            surface_config,
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            rect_pipeline,
            uniform_buffer,
            uniform_bind_group,
            grid: CellGrid::new(cols, rows),
            cursor: None,
            cell_w,
            cell_h,
        }
    }

    fn probe_cell_width(font_system: &mut FontSystem, font_size: f32) -> f32 {
        let metrics = Metrics::new(font_size, (font_size * 1.4).ceil());
        let mut buf = GlyphonBuffer::new(font_system, metrics);
        buf.set_text(
            font_system,
            "M",
            &Attrs::new().family(Family::Monospace),
            Shaping::Basic,
            None,
        );
        buf.shape_until_scroll(font_system, false);
        buf.layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map_or(font_size * 0.6, |g| g.w)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[width as f32, height as f32]),
        );
        self.viewport.update(
            &self.queue,
            Resolution {
                width,
                height,
            },
        );
    }

    pub fn apply_ops(&mut self, ops: &[DiffOp]) {
        for op in ops {
            match op {
                DiffOp::Resize { width, height } => {
                    self.grid = CellGrid::new(*width, *height);
                    self.cursor = None;
                }
                DiffOp::SetCell { x, y, cell } => {
                    self.grid.set(*x, *y, cell.clone());
                }
                DiffOp::MoveCursor(cr) => {
                    self.cursor = Some(*cr);
                }
                DiffOp::HideCursor => {
                    self.cursor = None;
                }
            }
        }
    }

    pub fn grid_size_cells(&self) -> (u16, u16) {
        let cols = (self.surface_config.width as f32 / self.cell_w).floor() as u16;
        let rows = (self.surface_config.height as f32 / self.cell_h).floor() as u16;
        (cols.max(1), rows.max(1))
    }

    /// Rasterise the current grid to the window surface. Returns
    /// `true` on success, `false` if the surface needs reconfiguring.
    #[allow(clippy::too_many_lines, clippy::similar_names)]
    pub fn render(&mut self) -> bool {
        let (wgpu::CurrentSurfaceTexture::Success(output)
        | wgpu::CurrentSurfaceTexture::Suboptimal(output)) =
            self.surface.get_current_texture()
        else {
            return false;
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let theme_bg = ResolvedFace::DEFAULT.bg;
        let theme_fg = ResolvedFace::DEFAULT.fg;

        // ---- Background rects ----
        let mut rects: Vec<RectInstance> = Vec::new();
        let gw = self.grid.width();
        let gh = self.grid.height();
        for (x, y, cell) in self.grid.iter() {
            if x >= gw || y >= gh {
                continue;
            }
            if cell.face.bg != theme_bg {
                rects.push(RectInstance {
                    pos: [f32::from(x) * self.cell_w, f32::from(y) * self.cell_h],
                    size: [self.cell_w, self.cell_h],
                    color: arx_color_to_f32(cell.face.bg),
                });
            }
        }
        if let Some(cr) = self.cursor {
            rects.push(RectInstance {
                pos: [
                    f32::from(cr.col) * self.cell_w,
                    f32::from(cr.row) * self.cell_h,
                ],
                size: [self.cell_w, self.cell_h],
                color: arx_color_to_f32(theme_fg),
            });
        }

        let rect_buffer = if rects.is_empty() {
            None
        } else {
            Some(
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("rect_instances"),
                        contents: bytemuck::cast_slice(&rects),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };

        // ---- Build glyphon text ----
        let font_size = 16.0_f32;
        let metrics = Metrics::new(font_size, self.cell_h);

        let mut text_buf = GlyphonBuffer::new(&mut self.font_system, metrics);
        text_buf.set_size(
            &mut self.font_system,
            Some(self.surface_config.width as f32),
            Some(self.surface_config.height as f32),
        );

        // Build per-cell spans so each cell's fg colour, bold, and
        // italic from the editor (syntax highlighting, modeline,
        // diagnostics, etc.) are preserved. Consecutive cells with
        // the same style are coalesced into a single span.
        let mut spans: Vec<(String, SpanStyle)> = Vec::new();
        for row in 0..gh {
            let mut run_text = String::new();
            let mut run_style = SpanStyle {
                fg: theme_fg,
                bold: false,
                italic: false,
            };
            for col in 0..gw {
                if let Some(cell) = self.grid.get(col, row) {
                    if cell
                        .flags
                        .contains(arx_render::CellFlags::WIDE_CONTINUATION)
                    {
                        continue;
                    }
                    let fg = cell.face.fg;
                    let bold = cell.face.bold;
                    let italic = cell.face.italic;
                    if !run_style.matches(fg, bold, italic) && !run_text.is_empty() {
                        spans.push((std::mem::take(&mut run_text), run_style));
                        run_style = SpanStyle { fg, bold, italic };
                    }
                    run_text.push_str(&cell.grapheme);
                } else {
                    run_text.push(' ');
                }
            }
            run_text.push('\n');
            spans.push((
                std::mem::take(&mut run_text),
                run_style,
            ));
        }

        let default_attrs = Attrs::new().family(Family::Monospace);
        let rich: Vec<(&str, Attrs)> = spans
            .iter()
            .map(|(text, style)| {
                let mut attrs = Attrs::new()
                    .family(Family::Monospace)
                    .color(arx_to_glyphon_color(style.fg));
                if style.bold {
                    attrs = attrs.weight(Weight::BOLD);
                }
                if style.italic {
                    attrs = attrs.style(Style::Italic);
                }
                (text.as_str(), attrs)
            })
            .collect();
        text_buf.set_rich_text(
            &mut self.font_system,
            rich,
            &default_attrs,
            Shaping::Basic,
            None,
        );
        text_buf.shape_until_scroll(&mut self.font_system, false);

        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                [TextArea {
                    buffer: &text_buf,
                    left: 0.0,
                    top: 0.0,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: self.surface_config.width as i32,
                        bottom: self.surface_config.height as i32,
                    },
                    default_color: arx_to_glyphon_color(theme_fg),
                    custom_glyphs: &[],
                }],
                &mut self.swash_cache,
            )
            .expect("glyphon prepare failed");

        // ---- Encode render passes ----
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gui_frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gui_bg_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(arx_to_wgpu_color(theme_bg)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(buf) = &rect_buffer {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_bind_group(0, Some(&self.uniform_bind_group), &[]);
                pass.set_vertex_buffer(0, buf.slice(..));
                for i in 0..rects.len() as u32 {
                    pass.draw(0..4, i..i + 1);
                }
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gui_text_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .unwrap();
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        self.atlas.trim();
        true
    }
}
