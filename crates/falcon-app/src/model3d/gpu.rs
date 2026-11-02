//! Drawing the 3D model with wgpu inside egui, through an
//! `egui_wgpu::CallbackTrait` paint callback.
//!
//! `prepare` renders the scene into the callback's own textures (4×
//! multisampled colour and depth, resolved to one texture the size of the
//! view in pixels): faces lit as in `geom::shade`, pushed back a little in
//! depth so edges drawn on them stay visible, then the edge, grid and axis
//! lines. `paint` then copies that texture into egui's pass over the
//! view's rectangle with premultiplied-alpha blending, so a transparent
//! background shows the viewer behind it.
//!
//! Textured meshes draw with a second pipeline: each texture is uploaded
//! once per model as sRGB with its mipmaps (made on the CPU in linear
//! light, `texture::mip_chain`), sampled trilinearly with anisotropic
//! filtering and the file's wrap modes, multiplied with the vertex colour
//! (the tint) in linear light, and discarded under its alpha cutoff. The
//! textures are dropped with the model ([`release`]) or when the next
//! model's arrive. Everything here is thin glue;
//! the maths it draws with is tested in `logic/viewer3d.rs`,
//! `model3d/mod.rs` and `falcon_library::model3d`.

use std::ops::Range;

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use falcon_library::model3d::geom;
use falcon_library::model3d::math::M4;

use super::{Lines, MeshData, VERTEX_FLOATS, Vertices};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;

const SCENE_SHADER: &str = r#"
struct U {
    view_proj: mat4x4<f32>,
    // xyz: unit direction towards the light (the eye).
    light: vec4<f32>,
    // ambient, diffuse, specular strength, shininess.
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) cutoff: f32,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) cutoff: f32,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    o.uv = v.uv;
    o.cutoff = v.cutoff;
    return o;
}

// The same lighting as falcon_library::model3d::geom::shade: both sides lit.
fn lit(normal: vec3<f32>, color: vec3<f32>) -> vec4<f32> {
    var d = 1.0;
    if (length(normal) > 1e-8) {
        d = abs(dot(normalize(normal), u.light.xyz));
    }
    let spec = u.params.z * pow(d, u.params.w);
    return vec4<f32>(min(color * (u.params.x + u.params.y * d) + vec3<f32>(spec), vec3<f32>(1.0)), 1.0);
}

@fragment fn fs_mesh(i: VOut) -> @location(0) vec4<f32> {
    return lit(i.normal, i.color);
}

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let k = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(k, vec3<f32>(1.0 / 2.4)) - 0.055, k * 12.92, k <= vec3<f32>(0.0031308));
}

// The picture (an sRGB texture, so sampled in linear light) times the
// tint, back in display values, then lit like the plain faces; as
// falcon_library::model3d::texture::modulate.
@fragment fn fs_tex(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    if (t.a < i.cutoff) {
        discard;
    }
    return lit(i.normal, to_srgb(to_linear(i.color) * t.rgb));
}

@fragment fn fs_line(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
"#;

const BLIT_SHADER: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct BOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_blit(@builtin(vertex_index) i: u32) -> BOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn to_linear(c: f32) -> f32 {
    if (c <= 0.04045) { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment fn fs_blit(i: BOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv);
    if (SRGB_TARGET && c.a > 0.0) {
        let rgb = c.rgb / c.a;
        return vec4<f32>(to_linear(rgb.r) * c.a, to_linear(rgb.g) * c.a, to_linear(rgb.b) * c.a, c.a);
    }
    return c;
}
"#;

/// One frame of the scene, as the UI hands it over.
pub(crate) struct SceneCallback {
    /// Which view draws: 0 for the main viewer, 1 for the preview windows.
    /// Each keeps its own GPU objects, so the main viewer and a preview
    /// window showing models don't upload over each other every frame.
    pub slot: u64,
    /// egui's framebuffer format.
    pub target: wgpu::TextureFormat,
    /// The view's size in physical pixels.
    pub size: [u32; 2],
    /// The background, premultiplied, in display values.
    pub clear: [f64; 4],
    pub view_proj: M4,
    /// Unit direction from the scene towards the eye.
    pub light: [f32; 3],
    pub mesh: Option<MeshData>,
    pub lines: Option<Lines>,
    pub edges: bool,
    pub grid: bool,
    pub axes: bool,
}

impl SceneCallback {
    pub fn shape(self, rect: eframe::egui::Rect) -> eframe::egui::Shape {
        eframe::egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, self))
    }
}

struct Buffer {
    generation: u64,
    buffer: wgpu::Buffer,
    count: u32,
}

/// One model texture on the GPU.
struct GpuTexture {
    _texture: wgpu::Texture,
    group: wgpu::BindGroup,
}

/// Textures the size of the view.
struct Targets {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

/// The GPU objects of each view (by [`SceneCallback::slot`]), kept in
/// egui's callback resources across frames. Every window shares egui's
/// one wgpu device, so a preview window's scene draws the same way.
#[derive(Default)]
struct Slots(std::collections::HashMap<u64, Gpu>);

/// The GPU objects of one view.
struct Gpu {
    target: wgpu::TextureFormat,
    uniform: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    mesh_pipeline: wgpu::RenderPipeline,
    tex_pipeline: wgpu::RenderPipeline,
    tex_layout: wgpu::BindGroupLayout,
    line_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
    mesh: Option<Buffer>,
    lines: Option<Buffer>,
    /// The model's textures, by the generation of their list.
    textures: Option<(u64, Vec<Option<GpuTexture>>)>,
}

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x2, 4 => Float32];
    wgpu::VertexBufferLayout { array_stride: (VERTEX_FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRS }
}

impl Gpu {
    fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Gpu {
        let scene = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("falcon-model3d-scene"),
            source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
        });
        let srgb = if target.is_srgb() { "true" } else { "false" };
        let blit = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("falcon-model3d-blit"),
            source: wgpu::ShaderSource::Wgsl(format!("const SRGB_TARGET: bool = {srgb};\n{BLIT_SHADER}").into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("falcon-model3d-uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("falcon-model3d-uniform"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("falcon-model3d-uniform"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("falcon-model3d-scene"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("falcon-model3d-texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let tex_scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("falcon-model3d-textured"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let scene_pipeline = |layout: &wgpu::PipelineLayout, topology: wgpu::PrimitiveTopology, fs: &str, bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("falcon-model3d"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &scene,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex_layout())],
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &scene,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Faces sit slightly behind lines at the same depth.
        let face_bias = wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 };
        let mesh_pipeline = scene_pipeline(&scene_layout, wgpu::PrimitiveTopology::TriangleList, "fs_mesh", face_bias);
        let tex_pipeline = scene_pipeline(&tex_scene_layout, wgpu::PrimitiveTopology::TriangleList, "fs_tex", face_bias);
        let line_pipeline = scene_pipeline(&scene_layout, wgpu::PrimitiveTopology::LineList, "fs_line", Default::default());
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("falcon-model3d-blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("falcon-model3d-blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        // egui's own pass: its format, one sample, no depth.
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("falcon-model3d-blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState { module: &blit, entry_point: Some("vs_blit"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit,
                entry_point: Some("fs_blit"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("falcon-model3d-blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Gpu {
            target,
            uniform,
            uniform_group,
            mesh_pipeline,
            tex_pipeline,
            tex_layout,
            line_pipeline,
            blit_pipeline,
            blit_layout,
            sampler,
            targets: None,
            mesh: None,
            lines: None,
            textures: None,
        }
    }

    /// Uploads the model's textures unless they are already there.
    fn ensure_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &super::Textures) {
        if self.textures.as_ref().is_some_and(|(g, _)| *g == t.generation) {
            return;
        }
        // The previous model's go first.
        self.textures = None;
        let list = t.list.iter().map(|tex| upload_texture(device, queue, &self.tex_layout, tex)).collect();
        self.textures = Some((t.generation, list));
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa = texture("falcon-model3d-msaa", COLOR_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = texture("falcon-model3d-depth", DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve =
            texture("falcon-model3d-resolve", COLOR_FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("falcon-model3d-blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolve) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.targets = Some(Targets { size, msaa, depth, resolve, blit_group });
    }
}

/// Uploads `v` into `slot` unless it already holds that generation.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut Option<Buffer>, v: &Vertices) {
    if slot.as_ref().is_some_and(|b| b.generation == v.generation) {
        return;
    }
    if v.bytes.is_empty() {
        *slot = None;
        return;
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("falcon-model3d-vertices"),
        size: v.bytes.len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &v.bytes);
    *slot = Some(Buffer { generation: v.generation, buffer, count: v.count });
}

fn address(w: falcon_library::model3d::texture::Wrap) -> wgpu::AddressMode {
    use falcon_library::model3d::texture::Wrap;
    match w {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    }
}

/// One texture as an sRGB GPU texture with its mipmaps, and its bind
/// group (texture and sampler). `None` for a picture the device can't hold.
fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, tex: &falcon_library::model3d::texture::Texture) -> Option<GpuTexture> {
    let img = &*tex.image;
    let max = device.limits().max_texture_dimension_2d;
    if img.width == 0 || img.height == 0 || img.width > max || img.height > max {
        return None;
    }
    let chain = falcon_library::model3d::texture::mip_chain(img);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("falcon-model3d-texture"),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: chain.len() as u32 + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let levels = std::iter::once((img.width, img.height, &img.rgba)).chain(chain.iter().map(|(w, h, px)| (*w, *h, px)));
    for (level, (w, h, px)) in levels.enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("falcon-model3d-texture"),
        address_mode_u: address(tex.wrap[0]),
        address_mode_v: address(tex.wrap[1]),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("falcon-model3d-texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    });
    Some(GpuTexture { _texture: texture, group })
}

/// Drops every view's GPU objects (vertex buffers, textures); the next
/// frame that draws a model makes them again.
pub(crate) fn release(render: &egui_wgpu::RenderState) {
    if let Some(slots) = render.renderer.write().callback_resources.get_mut::<Slots>() {
        slots.0.clear();
    }
}

const UNIFORM_BYTES: usize = 96;

fn uniform_bytes(view_proj: &M4, light: [f32; 3]) -> Vec<u8> {
    let mut floats: Vec<f32> = view_proj.iter().flatten().copied().collect();
    floats.extend_from_slice(&[light[0], light[1], light[2], 0.0]);
    floats.extend_from_slice(&[geom::AMBIENT, geom::DIFFUSE, geom::SPECULAR, geom::SHININESS as f32]);
    floats.iter().flat_map(|f| f.to_ne_bytes()).collect()
}

impl CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if self.size[0] == 0 || self.size[1] == 0 {
            return Vec::new();
        }
        if resources.get::<Slots>().is_none() {
            resources.insert(Slots::default());
        }
        let Some(slots) = resources.get_mut::<Slots>() else { return Vec::new() };
        if slots.0.get(&self.slot).is_none_or(|g| g.target != self.target) {
            slots.0.insert(self.slot, Gpu::new(device, self.target));
        }
        let Some(gpu) = slots.0.get_mut(&self.slot) else { return Vec::new() };
        gpu.ensure_targets(device, self.size);
        match &self.mesh {
            Some(m) => {
                upload(device, queue, &mut gpu.mesh, &m.vertices);
                gpu.ensure_textures(device, queue, &m.textures);
            }
            None => {
                gpu.mesh = None;
                gpu.textures = None;
            }
        }
        match &self.lines {
            Some(l) => upload(device, queue, &mut gpu.lines, &l.vertices),
            None => gpu.lines = None,
        }
        queue.write_buffer(&gpu.uniform, 0, &uniform_bytes(&self.view_proj, self.light));
        let Some(t) = &gpu.targets else { return Vec::new() };
        let [r, g, b, a] = self.clear;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("falcon-model3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &t.msaa,
                depth_slice: None,
                resolve_target: Some(&t.resolve),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &gpu.uniform_group, &[]);
        if let (Some(m), Some(data)) = (&gpu.mesh, &self.mesh) {
            pass.set_vertex_buffer(0, m.buffer.slice(..));
            for d in data.draws.iter().filter(|d| d.range.end <= m.count && !d.range.is_empty()) {
                let group = d.texture.and_then(|i| gpu.textures.as_ref()?.1.get(i)?.as_ref());
                match group {
                    Some(t) => {
                        pass.set_pipeline(&gpu.tex_pipeline);
                        pass.set_bind_group(1, &t.group, &[]);
                    }
                    // Untextured, or a texture the device couldn't take:
                    // the vertex colour (the tint) stands in.
                    None => pass.set_pipeline(&gpu.mesh_pipeline),
                }
                pass.draw(d.range.clone(), 0..1);
            }
        }
        if let (Some(buf), Some(l)) = (&gpu.lines, &self.lines) {
            let ranges: [(bool, &Range<u32>); 3] = [(self.edges, &l.edges), (self.grid, &l.grid), (self.axes, &l.axes)];
            pass.set_pipeline(&gpu.line_pipeline);
            pass.set_vertex_buffer(0, buf.buffer.slice(..));
            for (on, r) in ranges {
                if on && !r.is_empty() && r.end <= buf.count {
                    pass.draw(r.clone(), 0..1);
                }
            }
        }
        Vec::new()
    }

    fn paint(&self, _info: eframe::egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &CallbackResources) {
        let Some(gpu) = resources.get::<Slots>().and_then(|s| s.0.get(&self.slot)) else { return };
        let Some(t) = &gpu.targets else { return };
        if t.size != self.size {
            return;
        }
        pass.set_pipeline(&gpu.blit_pipeline);
        pass.set_bind_group(0, &t.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniforms_fill_the_buffer() {
        let id = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let b = uniform_bytes(&id, [0.0, 0.0, 1.0]);
        assert_eq!(b.len(), UNIFORM_BYTES);
        // Column-major: the first column first.
        assert_eq!(&b[0..4], &1.0f32.to_ne_bytes());
        assert_eq!(&b[4..8], &0.0f32.to_ne_bytes());
    }

    /// Runs a future that wgpu resolves without an event loop.
    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    /// Reads back an Rgba8Unorm texture of `size` pixels.
    fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<u8> {
        let row = (size[0] * 4).next_multiple_of(256);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        buf.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buf.slice(..).get_mapped_range().expect("mapped");
        (0..size[1]).flat_map(|y| data[(y * row) as usize..(y * row + size[0] * 4) as usize].to_vec()).collect()
    }

    /// The cube from (0,0,0) to (1,1,1) drawn through `prepare` and
    /// `paint` from the home direction on a 64 x 48 target, each face
    /// mapped over the whole of `texture` when one is given. Its pixels and
    /// size; `None` on machines without a GPU adapter.
    fn draw_cube(texture: Option<falcon_library::model3d::texture::Texture>) -> Option<(Vec<u8>, [u32; 2])> {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no GPU adapter; skipping");
            return None;
        };
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        // The blit shader also builds for an sRGB framebuffer.
        let _ = Gpu::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
        let size = [64u32, 48];
        let target_format = wgpu::TextureFormat::Rgba8Unorm;
        // The cube from (0,0,0) to (1,1,1), seen from the home direction.
        let mut cube = falcon_library::model3d::Mesh::default();
        let c: [[f32; 3]; 8] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]];
        cube.positions = c.to_vec();
        for q in [[0u32, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]] {
            cube.indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
        if texture.is_some() {
            // Corners repeated per triangle, so each face has its own UVs.
            let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            cube.positions = cube.indices.iter().map(|&i| c[i as usize]).collect();
            cube.indices = (0..36).collect();
            cube.uvs = Some((0..36).map(|i| quad[i % 6]).collect());
            cube.texture = texture;
        }
        let model = falcon_library::model3d::Model { meshes: vec![cube] };
        let mut camera = crate::logic::Camera::home(crate::logic::UpAxis::Y);
        let aspect = size[0] as f32 / size[1] as f32;
        camera.fit([0.5; 3], 0.75f32.sqrt(), aspect);
        let proj = camera.projection(crate::logic::Projection::Perspective, aspect, 0.75f32.sqrt());
        let mut lines = Vec::new();
        super::super::line_vertices(&mut lines, &[[[0.0; 3], [1.0, 0.0, 0.0]]], [1.0, 0.0, 0.0]);
        let cb = SceneCallback {
            slot: 0,
            target: target_format,
            size,
            clear: [0.0; 4],
            view_proj: falcon_library::model3d::math::mul(&proj, &camera.view()),
            light: camera.light_direction(),
            mesh: Some({
                let textures = super::super::Textures::of(&model);
                let (floats, draws) = super::super::mesh_data(&model, super::super::Shading::Flat, [0.8; 3], &[], &textures.list);
                MeshData { vertices: Vertices::new(&floats), draws: std::sync::Arc::new(draws), textures }
            }),
            lines: Some(Lines { vertices: Vertices::new(&lines), edges: 0..2, grid: 2..2, axes: 2..2 }),
            edges: true,
            grid: false,
            axes: false,
        };
        let mut resources = CallbackResources::default();
        let screen = ScreenDescriptor { size_in_pixels: size, pixels_per_point: 1.0 };
        let mut enc = device.create_command_encoder(&Default::default());
        let extra = cb.prepare(&device, &queue, &screen, &mut enc, &mut resources);
        assert!(extra.is_empty());
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        {
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            let r = eframe::egui::Rect::from_min_size(Default::default(), eframe::egui::vec2(size[0] as f32, size[1] as f32));
            let info = eframe::egui::PaintCallbackInfo { viewport: r, clip_rect: r, pixels_per_point: 1.0, screen_size_px: size };
            cb.paint(info, &mut pass, &resources);
        }
        queue.submit([enc.finish()]);
        Some((read(&device, &queue, &target, size), size))
    }

    /// The whole path on a real GPU, when there is one: a cube drawn by
    /// `prepare` and copied by `paint` lands in the middle of the target,
    /// lit grey, with the transparent background around it. Skipped on
    /// machines without an adapter.
    #[test]
    fn draws_a_cube_when_a_gpu_is_present() {
        let Some((px, size)) = draw_cube(None) else { return };
        let at = |x: u32, y: u32| {
            let i = ((y * size[0] + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let mid = at(size[0] / 2, size[1] / 2);
        assert_eq!(mid[3], 255, "the cube covers the middle: {mid:?}");
        assert!(mid[0] > 100 && mid[0] == mid[1] && mid[1] == mid[2], "lit grey: {mid:?}");
        assert_eq!(at(0, 0)[3], 0, "the background stays transparent");
        assert_eq!(at(size[0] - 1, size[1] - 1)[3], 0);
    }

    /// A textured cube shows its picture's colour (through the sRGB
    /// texture, so the shade matches a plain face of that colour), and a
    /// picture under its cutoff is not drawn.
    #[test]
    fn draws_a_textured_cube_when_a_gpu_is_present() {
        use falcon_library::model3d::texture::{Image, Texture};
        let image = std::sync::Arc::new(Image { name: "t".into(), width: 8, height: 8, rgba: [0u8, 0, 255, 255].repeat(64) });
        let Some((px, size)) = draw_cube(Some(Texture::new(image.clone()))) else { return };
        let i = ((size[1] / 2 * size[0] + size[0] / 2) * 4) as usize;
        let mid = &px[i..i + 4];
        assert!(mid[3] == 255 && mid[2] > 100 && mid[0] < 5 && mid[1] < 5, "blue from the picture: {mid:?}");
        let faint = std::sync::Arc::new(Image { rgba: [0u8, 0, 255, 10].repeat(64), ..(*image).clone() });
        let (px, _) = draw_cube(Some(Texture { alpha_cutoff: Some(0.5), ..Texture::new(faint) })).unwrap();
        assert_eq!(px[i + 3], 0, "cut out");
    }
}
