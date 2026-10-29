// The whole GPU render path is one full-screen triangle and one fragment
// shader. The UI is pre-rasterised by tiny-skia into a premultiplied RGBA
// texture, so this shader only has to put the wallpaper underneath it.
//
// Everything stays in sRGB space on purpose. The textures are Rgba8Unorm and
// the surface is a non-sRGB format, so sampling and blending match the CPU
// canvas bit for bit instead of round-tripping through linear space.

struct Globals {
  top: vec4<f32>,
  bottom: vec4<f32>,
  screen: vec2<f32>,
  bg_size: vec2<f32>,
  prev_size: vec2<f32>,
  opacity: f32,
  mode: u32,
  has_prev: u32,
  pad: u32,
  tail: vec2<f32>,
};

@group(0) @binding(0) var bg_tex: texture_2d<f32>;
@group(0) @binding(1) var prev_tex: texture_2d<f32>;
@group(0) @binding(2) var ui_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<uniform> globals: Globals;

struct VertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0),
  );
  let p = positions[index];
  var out: VertexOut;
  out.position = vec4<f32>(p, 0.0, 1.0);
  // The surface and all textures use a top-left origin, so flip y here rather
  // than uploading bottom-up pixels.
  out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
  return out;
}

// Mirrors Canvas::image_cover on the CPU: scale by the larger axis ratio and
// centre the crop. A frame that is already output-sized maps to uv unchanged.
fn cover_uv(uv: vec2<f32>, image: vec2<f32>) -> vec2<f32> {
  let scale = max(globals.screen.x / image.x, globals.screen.y / image.y);
  let size = image * scale;
  let offset = (globals.screen - size) * 0.5;
  return (uv * globals.screen - offset) / size;
}

fn wallpaper(uv: vec2<f32>) -> vec3<f32> {
  if (globals.mode == 1u) {
    return globals.top.rgb;
  }
  if (globals.mode == 2u) {
    return mix(globals.top.rgb, globals.bottom.rgb, clamp(uv.y, 0.0, 1.0));
  }
  let current = textureSample(bg_tex, samp, cover_uv(uv, globals.bg_size)).rgb;
  if (globals.has_prev == 0u) {
    return current;
  }
  let previous = textureSample(prev_tex, samp, cover_uv(uv, globals.prev_size)).rgb;
  return mix(previous, current, clamp(globals.opacity, 0.0, 1.0));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  let bg = wallpaper(in.uv);
  let ui = textureSample(ui_tex, samp, in.uv);
  // The UI pixmap is premultiplied, which is exactly the form this blend
  // wants: rgb is already scaled by alpha, so only the backdrop needs 1-a.
  return vec4<f32>(ui.rgb + bg * (1.0 - ui.a), 1.0);
}
