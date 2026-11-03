//! Drawing the 3D model with wgpu inside egui, through an
//! `egui_wgpu::CallbackTrait` paint callback.
//!
//! `prepare` renders the scene into the callback's own textures (4×
//! multisampled colour and depth, resolved to one texture the size of the
//! view in pixels): faces lit as in `geom::shade`, pushed back a little in
//! depth so edges drawn on them stay visible, then the edge, grid and axis
//! lines. `paint` then copies that texture into egui's pass over the
//! view's rectangle with premultiplied-alpha blending, so a transparent
//! background shows the viewer behind it. The grayscale preview desaturates
//! in that copy, so it covers the whole pane as Eagle's CSS filter does on its
//! iframe, and leaves snapshots and thumbnails, which never pass through it,
//! in colour.
//!
//! Meshes with a look draw with a second pipeline: each picture is uploaded
//! once per model as sRGB with its mipmaps (made on the CPU in linear
//! light, `texture::mip_chain`), sampled trilinearly with anisotropic
//! filtering and the file's wrap modes, multiplied with the vertex colour
//! (the tint) in linear light, and discarded under its alpha cutoff. Normal,
//! bump and emissive maps are uploaded as they are stored, with plain mipmaps
//! (`texture::mip_chain_data`); no tangents are stored, the frame comes from
//! screen derivatives. A look's missing maps are a white pixel the shader
//! ignores or multiplies with. The textures are dropped with the model
//! ([`release`]) or when the next model's arrive.
//!
//! See-through meshes draw last, farthest first, back faces and then front
//! faces, writing depth as they go, as three.js does in Eagle's viewer. The
//! target holds premultiplied colour, and the blend keeps it that way.
//! Everything here is thin glue;
//! the maths it draws with is tested in `logic/viewer3d.rs`,
//! `model3d/mod.rs` and `falcon_library::model3d`.

use std::ops::Range;
use std::sync::Arc;

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use falcon_library::model3d::geom;
use falcon_library::model3d::math::M4;
use falcon_library::model3d::texture::{self, Image, Texture, Wrap};

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
    @location(4) world: vec3<f32>,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    o.uv = v.uv;
    o.cutoff = v.cutoff;
    o.world = v.pos;
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

struct Look {
    // rgb: the surface's own light; a: its opacity.
    emissive: vec4<f32>,
    // x: has a normal map; y: has a bump map; z: see-through.
    flags: vec4<f32>,
};
@group(1) @binding(0) var<uniform> look: Look;
@group(1) @binding(1) var base_tex: texture_2d<f32>;
@group(1) @binding(2) var base_samp: sampler;
@group(1) @binding(3) var normal_tex: texture_2d<f32>;
@group(1) @binding(4) var normal_samp: sampler;
@group(1) @binding(5) var bump_tex: texture_2d<f32>;
@group(1) @binding(6) var bump_samp: sampler;
@group(1) @binding(7) var emissive_tex: texture_2d<f32>;
@group(1) @binding(8) var emissive_samp: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let k = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(k, vec3<f32>(1.0 / 2.4)) - 0.055, k * 12.92, k <= vec3<f32>(0.0031308));
}

// The surface's normal through its maps, as
// falcon_library::model3d::geom::normal_mapped and bumped, from screen
// derivatives. A normal map wins over a bump map.
fn surface_normal(n: vec3<f32>, q0: vec3<f32>, q1: vec3<f32>, st0: vec2<f32>, st1: vec2<f32>, front: bool, map: vec3<f32>, dh: vec2<f32>) -> vec3<f32> {
    if (length(n) <= 1e-8) {
        return n;
    }
    var m = normalize(n);
    let has_normal = look.flags.x > 0.5;
    if (has_normal) {
        if (!front) {
            m = -m;
        }
        let p1 = cross(q1, m);
        let p0 = cross(m, q0);
        let t = p1 * st0.x + p0 * st1.x;
        let b = p1 * st0.y + p0 * st1.y;
        let det = max(dot(t, t), dot(b, b));
        if (det <= 0.0) {
            return m;
        }
        let k = 1.0 / sqrt(det);
        let v = map * 2.0 - vec3<f32>(1.0);
        // v runs down the picture, the map's green up it.
        return normalize(t * (v.x * k) - b * (v.y * k) + m * v.z);
    }
    if (look.flags.y > 0.5) {
        let p1 = cross(q1, m);
        let p0 = cross(m, q0);
        let det = dot(q0, p1);
        if (abs(det) <= 0.0) {
            return m;
        }
        // dh is per screen pixel, and so are q0 and q1.
        return normalize(m - (p1 * dh.x + p0 * dh.y) / det);
    }
    return m;
}

// The picture (an sRGB texture, so sampled in linear light) times the
// tint, back in display values, then lit like the plain faces; as
// falcon_library::model3d::texture::modulate. Every map is read up front:
// derivatives aren't to be taken after a discard.
@fragment fn fs_tex(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = textureSample(base_tex, base_samp, i.uv);
    let map = textureSample(normal_tex, normal_samp, i.uv).xyz;
    let st0 = dpdx(i.uv);
    let st1 = dpdy(i.uv);
    let h = textureSample(bump_tex, bump_samp, i.uv).r;
    let hx = textureSample(bump_tex, bump_samp, i.uv + st0).r;
    let hy = textureSample(bump_tex, bump_samp, i.uv + st1).r;
    let own = textureSample(emissive_tex, emissive_samp, i.uv).rgb * look.emissive.rgb;
    let q0 = dpdx(i.world);
    let q1 = dpdy(i.world);
    let alpha = t.a * look.emissive.a;
    if (alpha < i.cutoff) {
        discard;
    }
    let n = surface_normal(i.normal, q0, q1, st0, st1, front, map, vec2<f32>(hx - h, hy - h));
    let lit_color = lit(n, to_srgb(to_linear(i.color) * t.rgb)).rgb;
    let a = select(1.0, alpha, look.flags.z > 0.5);
    return vec4<f32>(min(lit_color + own, vec3<f32>(1.0)), a);
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

fn output(c: vec4<f32>) -> vec4<f32> {
    if (SRGB_TARGET && c.a > 0.0) {
        let rgb = c.rgb / c.a;
        return vec4<f32>(to_linear(rgb.r) * c.a, to_linear(rgb.g) * c.a, to_linear(rgb.b) * c.a, c.a);
    }
    return c;
}

@fragment fn fs_blit(i: BOut) -> @location(0) vec4<f32> {
    return output(textureSample(tex, samp, i.uv));
}

// CSS's luminance weights on display values, as logic::grayscale_pixel.
@fragment fn fs_blit_gray(i: BOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv);
    let y = dot(c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return output(vec4<f32>(y, y, y, c.a));
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
    /// The owning window's grayscale preview is on.
    pub gray: bool,
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

/// The white pixels that stand in for a look's missing maps, and the samplers
/// by wrap mode, shared by all its bind groups.
struct LookParts {
    white: wgpu::TextureView,
    white_srgb: wgpu::TextureView,
    samplers: std::collections::HashMap<[Wrap; 2], wgpu::Sampler>,
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
    blend_back_pipeline: wgpu::RenderPipeline,
    blend_front_pipeline: wgpu::RenderPipeline,
    look_layout: wgpu::BindGroupLayout,
    line_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    blit_gray_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
    mesh: Option<Buffer>,
    lines: Option<Buffer>,
    /// The bind group of each of the model's looks, which hold on to its
    /// textures, by the generation of their list.
    looks: Option<(u64, Vec<wgpu::BindGroup>)>,
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
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let look_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("falcon-model3d-look"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex_entry(1),
                samp_entry(2),
                tex_entry(3),
                samp_entry(4),
                tex_entry(5),
                samp_entry(6),
                tex_entry(7),
                samp_entry(8),
            ],
        });
        let tex_scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("falcon-model3d-textured"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&look_layout)],
            immediate_size: 0,
        });
        let scene_pipeline = |layout: &wgpu::PipelineLayout, topology: wgpu::PrimitiveTopology, fs: &str, bias: wgpu::DepthBiasState, cull_mode: Option<wgpu::Face>, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("falcon-model3d"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &scene,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex_layout())],
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode, ..Default::default() },
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
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Faces sit slightly behind lines at the same depth.
        let face_bias = wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 };
        let triangles = wgpu::PrimitiveTopology::TriangleList;
        let mesh_pipeline = scene_pipeline(&scene_layout, triangles, "fs_mesh", face_bias, None, None);
        let tex_pipeline = scene_pipeline(&tex_scene_layout, triangles, "fs_tex", face_bias, None, None);
        // The shader's colour is straight; what lands in the target is
        // premultiplied, which is what the copy into egui's pass expects.
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let blend_back_pipeline = scene_pipeline(&tex_scene_layout, triangles, "fs_tex", face_bias, Some(wgpu::Face::Front), Some(over));
        let blend_front_pipeline = scene_pipeline(&tex_scene_layout, triangles, "fs_tex", face_bias, Some(wgpu::Face::Back), Some(over));
        let line_pipeline = scene_pipeline(&scene_layout, wgpu::PrimitiveTopology::LineList, "fs_line", Default::default(), None, None);
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
        let blit_for = |entry: &str| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("falcon-model3d-blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState { module: &blit, entry_point: Some("vs_blit"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit,
                entry_point: Some(entry),
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
        let (blit_pipeline, blit_gray_pipeline) = (blit_for("fs_blit"), blit_for("fs_blit_gray"));
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
            blend_back_pipeline,
            blend_front_pipeline,
            look_layout,
            line_pipeline,
            blit_pipeline,
            blit_gray_pipeline,
            blit_layout,
            sampler,
            targets: None,
            mesh: None,
            lines: None,
            looks: None,
        }
    }

    /// Uploads the model's pictures and makes its looks' bind groups unless
    /// they are already there. A picture used by several looks, or several
    /// times by one, goes up once.
    fn ensure_looks(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, looks: &super::Looks) {
        if self.looks.as_ref().is_some_and(|(g, _)| *g == looks.generation) {
            return;
        }
        // The previous model's go first.
        self.looks = None;
        let white = |format| {
            let image = falcon_library::model3d::texture::Image { name: String::new(), width: 1, height: 1, rgba: vec![255; 4] };
            upload_image(device, queue, &image, format).expect("a pixel fits")
        };
        let mut parts = LookParts { white: white(wgpu::TextureFormat::Rgba8Unorm), white_srgb: white(wgpu::TextureFormat::Rgba8UnormSrgb), samplers: Default::default() };
        let mut uploaded = std::collections::HashMap::new();
        let groups = looks.list.iter().map(|look| self.look_group(device, queue, look, &mut parts, &mut uploaded)).collect();
        self.looks = Some((looks.generation, groups));
    }

    fn look_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        look: &super::Look,
        parts: &mut LookParts,
        uploaded: &mut std::collections::HashMap<(*const Image, bool), Option<wgpu::TextureView>>,
    ) -> wgpu::BindGroup {
        let mut view = |t: &Option<Texture>, srgb: bool| -> Option<wgpu::TextureView> {
            let t = t.as_ref()?;
            let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
            uploaded.entry((Arc::as_ptr(&t.image), srgb)).or_insert_with(|| upload_image(device, queue, &t.image, format)).clone()
        };
        let (base, normal, bump, emissive) = (view(&look.base, true), view(&look.normal, false), view(&look.bump, false), view(&look.emissive_map, false));
        // A map that didn't make it to the GPU counts as missing.
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("falcon-model3d-look"),
            size: LOOK_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform, 0, &look_bytes(look.emissive, look.opacity, normal.is_some(), bump.is_some(), look.blend));
        for t in [&look.base, &look.normal, &look.bump, &look.emissive_map].into_iter().flatten() {
            parts.samplers.entry(t.wrap).or_insert_with(|| make_sampler(device, t.wrap));
        }
        parts.samplers.entry([Wrap::Repeat; 2]).or_insert_with(|| make_sampler(device, [Wrap::Repeat; 2]));
        let samplers = &parts.samplers;
        let sampler = |t: &Option<Texture>| &samplers[&t.as_ref().map_or([Wrap::Repeat; 2], |t| t.wrap)];
        let or_white = |v: &'_ Option<wgpu::TextureView>, srgb: bool| -> wgpu::TextureView { v.clone().unwrap_or_else(|| if srgb { parts.white_srgb.clone() } else { parts.white.clone() }) };
        let views = [or_white(&base, true), or_white(&normal, false), or_white(&bump, false), or_white(&emissive, false)];
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("falcon-model3d-look"),
            layout: &self.look_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler(&look.base)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&views[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler(&look.normal)) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&views[2]) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(sampler(&look.bump)) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&views[3]) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(sampler(&look.emissive_map)) },
            ],
        })
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

fn address(w: Wrap) -> wgpu::AddressMode {
    match w {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    }
}

/// A picture as a GPU texture with its mipmaps: in `format`, sRGB for a
/// picture and plain for a map holding data. `None` for one the device
/// can't hold.
fn upload_image(device: &wgpu::Device, queue: &wgpu::Queue, img: &Image, format: wgpu::TextureFormat) -> Option<wgpu::TextureView> {
    let max = device.limits().max_texture_dimension_2d;
    if img.width == 0 || img.height == 0 || img.width > max || img.height > max {
        return None;
    }
    let chain = if format.is_srgb() { texture::mip_chain(img) } else { texture::mip_chain_data(img) };
    let gpu = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("falcon-model3d-texture"),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: chain.len() as u32 + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let levels = std::iter::once((img.width, img.height, &img.rgba)).chain(chain.iter().map(|(w, h, px)| (*w, *h, px)));
    for (level, (w, h, px)) in levels.enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &gpu, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
    Some(gpu.create_view(&wgpu::TextureViewDescriptor::default()))
}

fn make_sampler(device: &wgpu::Device, wrap: [Wrap; 2]) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("falcon-model3d-texture"),
        address_mode_u: address(wrap[0]),
        address_mode_v: address(wrap[1]),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    })
}

/// Drops every view's GPU objects (vertex buffers, textures); the next
/// frame that draws a model makes them again.
pub(crate) fn release(render: &egui_wgpu::RenderState) {
    if let Some(slots) = render.renderer.write().callback_resources.get_mut::<Slots>() {
        slots.0.clear();
    }
}

const UNIFORM_BYTES: usize = 96;
const LOOK_BYTES: usize = 32;

fn look_bytes(emissive: [f32; 3], opacity: f32, normal: bool, bump: bool, blend: bool) -> Vec<u8> {
    let flag = |on: bool| if on { 1.0f32 } else { 0.0 };
    [emissive[0], emissive[1], emissive[2], opacity, flag(normal), flag(bump), flag(blend), 0.0].iter().flat_map(|f| f.to_ne_bytes()).collect()
}

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
                gpu.ensure_looks(device, queue, &m.looks);
            }
            None => {
                gpu.mesh = None;
                gpu.looks = None;
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
        let mesh = gpu.mesh.as_ref().zip(self.mesh.as_ref());
        let drawn = mesh.map_or(Vec::new(), |(m, data)| data.draws.iter().filter(|d| d.range.end <= m.count && !d.range.is_empty()).collect::<Vec<_>>());
        let group = |d: &super::Draw| d.look.and_then(|i| gpu.looks.as_ref()?.1.get(i));
        if let Some((m, _)) = mesh {
            pass.set_vertex_buffer(0, m.buffer.slice(..));
            for d in drawn.iter().filter(|d| !d.blend) {
                match group(d) {
                    Some(g) => {
                        pass.set_pipeline(&gpu.tex_pipeline);
                        pass.set_bind_group(1, g, &[]);
                    }
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
        if let Some((m, _)) = mesh {
            let blended: Vec<_> = drawn.iter().filter(|d| d.blend).collect();
            if !blended.is_empty() {
                pass.set_vertex_buffer(0, m.buffer.slice(..));
                let centers: Vec<_> = blended.iter().map(|d| d.center).collect();
                for i in geom::back_to_front(&centers, &self.view_proj) {
                    let Some(g) = group(blended[i]) else { continue };
                    pass.set_bind_group(1, g, &[]);
                    for pipeline in [&gpu.blend_back_pipeline, &gpu.blend_front_pipeline] {
                        pass.set_pipeline(pipeline);
                        pass.draw(blended[i].range.clone(), 0..1);
                    }
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
        pass.set_pipeline(if self.gray { &gpu.blit_gray_pipeline } else { &gpu.blit_pipeline });
        pass.set_bind_group(0, &t.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use falcon_library::model3d::Material;

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
    fn draw_cube(texture: Option<Texture>) -> Option<(Vec<u8>, [u32; 2])> {
        draw_cube_with(texture, |_| {})
    }

    /// [`draw_cube`] with the cube changed by `change` before it is drawn.
    fn draw_cube_with(texture: Option<Texture>, change: impl FnOnce(&mut falcon_library::model3d::Mesh)) -> Option<(Vec<u8>, [u32; 2])> {
        draw_cube_gray(texture, change, false)
    }

    /// [`draw_cube_with`], copied to the target in grayscale when `gray`.
    fn draw_cube_gray(texture: Option<Texture>, change: impl FnOnce(&mut falcon_library::model3d::Mesh), gray: bool) -> Option<(Vec<u8>, [u32; 2])> {
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
        // Corners repeated per triangle, so each face has its own UVs.
        let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        cube.positions = cube.indices.iter().map(|&i| c[i as usize]).collect();
        cube.indices = (0..36).collect();
        cube.uvs = Some((0..36).map(|i| quad[i % 6]).collect());
        cube.texture = texture;
        change(&mut cube);
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
                let looks = super::super::Looks::of(&model);
                let (floats, draws) = super::super::mesh_data(&model, super::super::Shading::Flat, [0.8; 3], &[], &looks.list);
                MeshData { vertices: Vertices::new(&floats), draws: Arc::new(draws), looks }
            }),
            lines: Some(Lines { vertices: Vertices::new(&lines), edges: 0..2, grid: 2..2, axes: 2..2 }),
            edges: true,
            grid: false,
            axes: false,
            gray,
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
        let image = Arc::new(Image { name: "t".into(), width: 8, height: 8, rgba: [0u8, 0, 255, 255].repeat(64) });
        let Some((px, size)) = draw_cube(Some(Texture::new(image.clone()))) else { return };
        let i = ((size[1] / 2 * size[0] + size[0] / 2) * 4) as usize;
        let mid = &px[i..i + 4];
        assert!(mid[3] == 255 && mid[2] > 100 && mid[0] < 5 && mid[1] < 5, "blue from the picture: {mid:?}");
        let faint = Arc::new(Image { rgba: [0u8, 0, 255, 10].repeat(64), ..(*image).clone() });
        let (px, _) = draw_cube(Some(Texture { alpha_cutoff: Some(0.5), ..Texture::new(faint) })).unwrap();
        assert_eq!(px[i + 3], 0, "cut out");
    }

    /// The middle pixel of the cube with `change` applied.
    fn middle(change: impl FnOnce(&mut falcon_library::model3d::Mesh)) -> Option<[u8; 4]> {
        let (px, size) = draw_cube_with(None, change)?;
        let i = ((size[1] / 2 * size[0] + size[0] / 2) * 4) as usize;
        Some([px[i], px[i + 1], px[i + 2], px[i + 3]])
    }

    fn solid(rgba: [u8; 4]) -> Texture {
        Texture::new(Arc::new(Image { name: "m".into(), width: 4, height: 4, rgba: rgba.repeat(16) }))
    }

    /// The grayscale pane: the colour the scene drew, through the weights
    /// the image viewers use, and the background still clear.
    #[test]
    fn grayscale_pane_when_a_gpu_is_present() {
        let at = |px: &[u8], x: u32, y: u32, w: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let texture = || Some(solid([255, 40, 0, 255]));
        let Some((color, size)) = draw_cube_gray(texture(), |_| {}, false) else { return };
        let Some((gray, _)) = draw_cube_gray(texture(), |_| {}, true) else { return };
        let (c, g) = (at(&color, size[0] / 2, size[1] / 2, size[0]), at(&gray, size[0] / 2, size[1] / 2, size[0]));
        assert!(c[0] > c[1] + 50, "the cube is in colour: {c:?}");
        assert_eq!((g[0], g[1], g[3]), (g[1], g[2], 255), "gray: {g:?}");
        let want = crate::logic::grayscale_pixel(c)[0];
        assert!(g[0].abs_diff(want) <= 1, "{g:?} against {want}");
        assert_eq!(at(&gray, 0, 0, size[0])[3], 0, "the background stays transparent");
    }

    /// Normal maps turn the lit face, a flat one changes nothing, and the
    /// bump map is ignored beside a normal map.
    #[test]
    fn normal_and_bump_maps_when_a_gpu_is_present() {
        let Some(plain) = middle(|_| {}) else { return };
        let flat = middle(|m| m.surface.normal_map = Some(solid([128, 128, 255, 255]))).unwrap();
        assert!(flat[0].abs_diff(plain[0]) <= 2, "{flat:?} against {plain:?}");
        let tilted = middle(|m| m.surface.normal_map = Some(solid([255, 128, 128, 255]))).unwrap();
        assert!(tilted[0].abs_diff(plain[0]) > 10, "{tilted:?} against {plain:?}");
        // A level height field leans nothing.
        let level = middle(|m| m.surface.bump_map = Some(solid([200, 0, 0, 255]))).unwrap();
        assert!(level[0].abs_diff(plain[0]) <= 2, "{level:?} against {plain:?}");
    }

    /// A height ramp across the picture leans the surface; a steeper one
    /// more so.
    #[test]
    fn bump_slopes_when_a_gpu_is_present() {
        let ramp = |steps: u32| {
            let px: Vec<u8> = (0..64u32).flat_map(|x| [(x * steps) as u8, 0, 0, 255]).collect();
            Texture { wrap: [Wrap::Clamp; 2], ..Texture::new(Arc::new(Image { name: "h".into(), width: 64, height: 1, rgba: px })) }
        };
        let Some(plain) = middle(|_| {}) else { return };
        let bumped = middle(|m| m.surface.bump_map = Some(ramp(4))).unwrap();
        assert!(bumped[0].abs_diff(plain[0]) > 3, "{bumped:?} against {plain:?}");
    }

    /// The surface's own light adds to the lit colour, through its map.
    #[test]
    fn own_light_when_a_gpu_is_present() {
        let dark = Material { name: "k".into(), color: [0.0; 3], opacity: 1.0 };
        let Some(off) = middle(|m| m.material = Some(dark.clone())) else { return };
        let on = middle(|m| {
            m.material = Some(dark.clone());
            m.surface.emissive = [1.0, 0.0, 0.0];
        })
        .unwrap();
        assert_eq!((on[0], on[1]), (255, off[1]), "{on:?}");
        let mapped = middle(|m| {
            m.material = Some(dark.clone());
            m.surface.emissive = [1.0, 0.0, 0.0];
            m.surface.emissive_map = Some(solid([51, 255, 255, 255]));
        })
        .unwrap();
        assert!(mapped[0].abs_diff(off[0] + 51) <= 2, "{mapped:?} over {off:?}");
    }

    /// A see-through box shows both walls over the clear background: the
    /// back faces go first and the front faces don't hide them. Its result
    /// stays premultiplied, as the copy into egui's pass takes it.
    #[test]
    fn see_through_box_when_a_gpu_is_present() {
        let green = Material { name: "g".into(), color: [0.0, 1.0, 0.0], opacity: 0.5 };
        let Some(p) = middle(|m| {
            m.material = Some(green);
            m.surface.blend = true;
        }) else {
            return;
        };
        assert!(p[3] > 180 && p[3] < 200, "two layers of a half: {p:?}");
        assert!(p[1] <= p[3] && p[1] > 90, "premultiplied green: {p:?}");
        // Opacity without the flag is ignored, as in Eagle.
        let solid_box = middle(|m| m.material = Some(Material { name: "g".into(), color: [0.0, 1.0, 0.0], opacity: 0.5 })).unwrap();
        assert_eq!(solid_box[3], 255);
        // Picture alpha counts when see-through and the cutoff still applies.
        let faint = middle(|m| {
            m.texture = Some(solid([0, 0, 255, 64]));
            m.surface.blend = true;
        })
        .unwrap();
        assert!(faint[3] > 90 && faint[3] < 125, "two layers of a quarter: {faint:?}");
        let cut = middle(|m| {
            m.texture = Some(Texture { alpha_cutoff: Some(0.5), ..solid([0, 0, 255, 64]) });
            m.surface.blend = true;
        })
        .unwrap();
        assert_eq!(cut[3], 0);
    }

    #[test]
    fn looks_fill_their_buffer() {
        let b = look_bytes([1.0, 0.5, 0.0], 0.25, true, false, true);
        assert_eq!(b.len(), LOOK_BYTES);
        assert_eq!(&b[12..16], &0.25f32.to_ne_bytes());
        assert_eq!(&b[16..20], &1.0f32.to_ne_bytes());
        assert_eq!(&b[24..28], &1.0f32.to_ne_bytes());
    }
}
