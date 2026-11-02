// The whole GPU render path is one full-screen triangle and one fragment
// shader. The UI is pre-rasterised by tiny-skia into a premultiplied RGBA
// texture, so this shader only has to put the wallpaper underneath it.
//
// Everything stays in sRGB space on purpose. The textures are Rgba8Unorm and
// the surface is a non-sRGB format, so sampling and blending match the CPU
// canvas bit for bit instead of round-tripping through linear space.
//
// ENCORE adds an every-frame camera transform plus two GPU-only post effects:
// chromatic aberration and glitch slice displacement. The CPU path deliberately
// skips those two and draws letterbox, speed lines, and vignette itself.

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
  // scale, dx in px, dy in px, unused
  camera: vec4<f32>,
  // chromatic px, vignette, glitch strength, glitch seed
  post: vec4<f32>,
  // splash underline: x, y, width, height in physical pixels
  splash: vec4<f32>,
  // straight RGBA for the underline
  splash_color: vec4<f32>,
  splash_on: u32,
  decor_on: u32,
  splash_pad1: u32,
  splash_pad2: u32,
};

@group(0) @binding(0) var bg_tex: texture_2d<f32>;
@group(0) @binding(1) var prev_tex: texture_2d<f32>;
@group(0) @binding(2) var ui_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var<uniform> globals: Globals;
@group(0) @binding(5) var decor_tex: texture_2d<f32>;

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

// One composited scene sample. The camera scale zooms around the centre and the
// shake offset is in physical pixels; converting it here keeps the CPU free of
// aspect-ratio math.
fn camera_uv(uv: vec2<f32>) -> vec2<f32> {
  let scale = max(globals.camera.x, 0.01);
  var p = (uv - vec2<f32>(0.5)) / scale + vec2<f32>(0.5);
  p = p + vec2<f32>(globals.camera.y / globals.screen.x, globals.camera.z / globals.screen.y);
  return p;
}

// Glitch slices: 3 to 6 horizontal bands, each displaced sideways by a stable
// hash. The seed changes every rendered frame, so the slices move like a bad
// signal rather than a fixed warp.
fn glitch_uv(uv: vec2<f32>) -> vec2<f32> {
  if (globals.post.z <= 0.0) {
    return uv;
  }
  let slices = 3.0 + floor(fract(globals.post.w * 0.371) * 4.0);
  let y = clamp(uv.y, 0.0, 0.9999);
  let band = floor(y * slices);
  let hash = fract(sin(band * 12.9898 + globals.post.w * 78.233) * 43758.5453);
  let shift = (hash - 0.5) * 0.05 * globals.post.z;
  return vec2<f32>(uv.x + shift, uv.y);
}

fn scene(uv: vec2<f32>) -> vec3<f32> {
  let bg = wallpaper(uv);
  // Static decor is a screen-sized premultiplied texture uploaded once. It
  // sits over the wallpaper and under the live form and animated ornaments.
  var decor = vec4<f32>(0.0);
  if (globals.decor_on != 0u) {
    decor = textureSample(decor_tex, samp, uv);
  }
  let base = decor.rgb + bg * (1.0 - decor.a);
  let ui = textureSample(ui_tex, samp, uv);
  // The UI pixmap is premultiplied, which is exactly the form this blend
  // wants: rgb is already scaled by alpha, so only the backdrop needs 1-a.
  return ui.rgb + base * (1.0 - ui.a);
}

// The CPU overlay paints the same vignette for the software fallback, but the
// GPU path computes it here. A full-screen radial gradient costs about 22 ms
// per frame on the CPU at 1920x1080, which was enough to drag video to 23 fps.
fn vignette(uv: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
  let strength = globals.post.y;
  if (strength <= 0.0) {
    return color;
  }
  let from_center = (uv - vec2<f32>(0.5)) * globals.screen;
  let radius = 0.5 * length(globals.screen);
  let r = length(from_center) / max(radius, 1.0);
  let edge = clamp((r - 0.55) / 0.45, 0.0, 1.0);
  return mix(color, vec3<f32>(0.0), edge * strength * (190.0 / 255.0));
}

// The splash card is a static texture. Only its underline moves, so the
// shader draws that line instead of forcing a full overlay upload every beat.
fn splash_underline(uv: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
  if (globals.splash_on == 0u) {
    return color;
  }
  let p = uv * globals.screen;
  let lo = globals.splash.xy;
  let hi = lo + globals.splash.zw;
  if (p.x >= lo.x && p.y >= lo.y && p.x < hi.x && p.y < hi.y) {
    return mix(color, globals.splash_color.rgb, globals.splash_color.a);
  }
  return color;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  let base = glitch_uv(camera_uv(in.uv));
  var color: vec3<f32>;
  let split = globals.post.x * 6.0 / max(globals.screen.x, 1.0);
  if (split > 0.0) {
    // Sample the red and blue channels at opposite horizontal offsets. The
    // green channel stays put, which is the usual cheap aberration look.
    let left = glitch_uv(camera_uv(in.uv + vec2<f32>(split, 0.0)));
    let right = glitch_uv(camera_uv(in.uv - vec2<f32>(split, 0.0)));
    color = vec3<f32>(scene(left).r, scene(base).g, scene(right).b);
  } else {
    color = scene(base);
  }
  color = splash_underline(in.uv, color);
  color = vignette(in.uv, color);
  return vec4<f32>(color, 1.0);
}
