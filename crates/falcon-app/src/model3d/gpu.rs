//! Drawing the 3D model with wgpu inside egui, through an
//! `egui_wgpu::CallbackTrait` paint callback.
//!
//! `prepare` renders the scene into the callback's own textures (4×
//! multisampled colour and depth, resolved to one texture the size of the
//! view in pixels): faces lit as in `geom::shade`, pushed back a little in
//! depth so edges drawn on them stay visible, then the edge, grid and axis
//! lines. `paint` then copies that texture into egui's pass over the
//! view's rectangle with premultiplied-alpha blending, so a transparent
//! background shows the viewer behind it. Everything here is thin glue;
//! the maths it draws with is tested in `logic/viewer3d.rs`,
//! `model3d/mod.rs` and `falcon_library::model3d`.

use std::ops::Range;

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use falcon_library::model3d::geom;
use falcon_library::model3d::math::M4;

use super::{Lines, VERTEX_FLOATS, Vertices};

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
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

// The same lighting as falcon_library::model3d::geom::shade: both sides lit.
@fragment fn fs_mesh(i: VOut) -> @location(0) vec4<f32> {
    var d = 1.0;
    if (length(i.normal) > 1e-8) {
        d = abs(dot(normalize(i.normal), u.light.xyz));
    }
    let spec = u.params.z * pow(d, u.params.w);
    return vec4<f32>(min(i.color * (u.params.x + u.params.y * d) + vec3<f32>(spec), vec3<f32>(1.0)), 1.0);
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
    pub mesh: Option<Vertices>,
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
    line_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
    mesh: Option<Buffer>,
    lines: Option<Buffer>,
}

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
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
        let scene_pipeline = |topology: wgpu::PrimitiveTopology, fs: &str, bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("falcon-model3d"),
                layout: Some(&scene_layout),
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
        let mesh_pipeline = scene_pipeline(wgpu::PrimitiveTopology::TriangleList, "fs_mesh", wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 });
        let line_pipeline = scene_pipeline(wgpu::PrimitiveTopology::LineList, "fs_line", Default::default());
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
            line_pipeline,
            blit_pipeline,
            blit_layout,
            sampler,
            targets: None,
            mesh: None,
            lines: None,
        }
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
            Some(v) => upload(device, queue, &mut gpu.mesh, v),
            None => gpu.mesh = None,
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
        if let Some(m) = &gpu.mesh {
            pass.set_pipeline(&gpu.mesh_pipeline);
            pass.set_vertex_buffer(0, m.buffer.slice(..));
            pass.draw(0..m.count, 0..1);
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

    /// The whole path on a real GPU, when there is one: a cube drawn by
    /// `prepare` and copied by `paint` lands in the middle of the target,
    /// lit grey, with the transparent background around it. Skipped on
    /// machines without an adapter.
    #[test]
    fn draws_a_cube_when_a_gpu_is_present() {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no GPU adapter; skipping");
            return;
        };
        let Ok((device, queue)) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())) else { return };
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
            mesh: Some(Vertices::new(&super::super::mesh_vertices(&model, super::super::Shading::Flat, [0.8; 3], &[]))),
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
        let px = read(&device, &queue, &target, size);
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
}
