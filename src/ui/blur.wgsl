// Separable Gaussian plus the whole glass colour pipeline.
//
// The CPU fallback runs a dual Kawase pyramid and finishes the pixels on the
// CPU. On the GPU we can do the real 33-tap Gaussian and, more importantly,
// keep the expensive per-pixel colour work in the shader too. Doing the colour
// math here is what keeps a 1080p refresh fast: the CPU only uploads the raw
// source and copies the finished rows back out.
//
// The split is deliberate:
//   horizontal pass: blur, then Hyprland's `blurprepare` and vibrancy
//   vertical pass:   blur, then the grain and final brightness (`blurFinish`)
//
// `uv` is top-left based for the same reason as the composite shader: the
// textures and readback rows both come from tiny-skia.

struct Params {
  // dir.x, dir.y, radius in output pixels, contrast
  dir_radius: vec4<f32>,
  // vibrancy, vibrancy_darkness, noise, brightness
  color: vec4<f32>,
  // finish (0 horizontal, 1 vertical), passes, unused, unused
  flags: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

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
  out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
  return out;
}

// `rem_euclid` is not a WGSL builtin, and `%` can go negative. The HSL helpers
// rely on the Euclidean version to keep the hue in [0, 1) and the sector in
// [0, 2).
fn rem_euclid(value: f32, modulus: f32) -> f32 {
  let remainder = value % modulus;
  return select(remainder + modulus, remainder, remainder >= 0.0);
}

// The gain curve from Hyprland's `gain.glsl`: a symmetric power curve around
// 0.5, which keeps black and white fixed and makes midtones punchier.
fn gain(value: f32, k: f32) -> f32 {
  let x = clamp(value, 0.0, 1.0);
  if (x < 0.5) {
    return 0.5 * pow(2.0 * x, k);
  }
  return 1.0 - 0.5 * pow(2.0 * (1.0 - x), k);
}

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
  let maximum = max(rgb.r, max(rgb.g, rgb.b));
  let minimum = min(rgb.r, min(rgb.g, rgb.b));
  let delta = maximum - minimum;
  let lightness = (maximum + minimum) / 2.0;
  var saturation = 0.0;
  if (delta > 0.0) {
    saturation = delta / (1.0 - abs(2.0 * lightness - 1.0));
  }
  var hue = 0.0;
  if (delta > 0.0) {
    if (maximum == rgb.r) {
      hue = rem_euclid((rgb.g - rgb.b) / delta, 6.0);
    } else if (maximum == rgb.g) {
      hue = (rgb.b - rgb.r) / delta + 2.0;
    } else {
      hue = (rgb.r - rgb.g) / delta + 4.0;
    }
  }
  return vec3<f32>(hue / 6.0, saturation, lightness);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
  let chroma = (1.0 - abs(2.0 * hsl.z - 1.0)) * hsl.y;
  let sector = hsl.x * 6.0;
  let x = chroma * (1.0 - abs(rem_euclid(sector, 2.0) - 1.0));
  var rgb = vec3<f32>(0.0);
  if (sector < 1.0) {
    rgb = vec3<f32>(chroma, x, 0.0);
  } else if (sector < 2.0) {
    rgb = vec3<f32>(x, chroma, 0.0);
  } else if (sector < 3.0) {
    rgb = vec3<f32>(0.0, chroma, x);
  } else if (sector < 4.0) {
    rgb = vec3<f32>(0.0, x, chroma);
  } else if (sector < 5.0) {
    rgb = vec3<f32>(x, 0.0, chroma);
  } else {
    rgb = vec3<f32>(chroma, 0.0, x);
  }
  let m = hsl.z - chroma / 2.0;
  return rgb + vec3<f32>(m);
}

fn smoothstep01(edge0: f32, edge1: f32, x: f32) -> f32 {
  let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

fn sigmoid(x: f32, a: f32) -> f32 {
  let aa = clamp(a, 0.0, 1.0);
  if (x <= aa) {
    return aa - sqrt(max(aa * aa - x * x, 0.0));
  }
  return aa + sqrt(max(pow(1.0 - aa, 2.0) - pow(x - 1.0, 2.0), 0.0));
}

// Hyprland's blur1 saturation boost. The source is opaque, so unlike the CPU
// version we never divide by alpha; every other step is the same.
fn apply_vibrancy(rgb_in: vec3<f32>, vibrancy: f32, darkness_in: f32, passes: f32) -> vec3<f32> {
  let hsl = rgb_to_hsl(rgb_in);
  let darkness = 1.0 - clamp(darkness_in, 0.0, 1.0);
  let perceived = sigmoid(
    sqrt(rgb_in.r * rgb_in.r * 0.299 + rgb_in.g * rgb_in.g * 0.587 + rgb_in.b * rgb_in.b * 0.114),
    0.8 * darkness,
  );
  let base = 0.11 * darkness;
  let angle = 0.93;
  var boost = 0.0;
  if (hsl.y > 0.0) {
    let value = 1.0 - pow(1.0 - hsl.y * cos(angle), 2.0) - pow(1.0 - perceived * sin(angle), 2.0);
    boost = smoothstep01(base - 0.33, base + 0.33, value);
  }
  let saturation = clamp(hsl.y + boost * vibrancy / max(passes, 1.0), 0.0, 1.0);
  return hsl_to_rgb(vec3<f32>(hsl.x, saturation, hsl.z));
}

// The same fractal hash as glass.rs, keyed by the normalized coordinate. It is
// not pixel-identical to the CPU grain, but it is fine-grained and stable across
// frames, which is all the finish pass needs.
fn hash_noise(uv: vec2<f32>) -> f32 {
  var p = vec3<f32>(
    fract(uv.x * 1689.1984),
    fract(uv.y * 1689.1984),
    fract(uv.x * 1689.1984),
  );
  let dot_value = p.x * (p.y + 33.33) + p.y * (p.z + 33.33) + p.z * (p.x + 33.33);
  p += vec3<f32>(dot_value);
  return fract((p.x + p.y) * p.z);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
  let texel = 1.0 / vec2<f32>(textureDimensions(src_tex));
  // A radius below one pixel would collapse to a single sample and look like a
  // nearest-neighbour copy. Half a pixel per tap keeps even `size = 1` soft.
  let radius = params.dir_radius.z;
  let tap_step = max(radius / 16.0, 0.5);
  let sigma = max(radius * 0.5, 0.5);
  let denom = 2.0 * sigma * sigma;
  let dir = params.dir_radius.xy;
  var sum = vec4<f32>(0.0);
  var total = 0.0;
  for (var i = -16; i <= 16; i = i + 1) {
    let offset = f32(i) * tap_step;
    let weight = exp(-(offset * offset) / denom);
    sum += textureSample(src_tex, samp, in.uv + dir * texel * offset) * weight;
    total += weight;
  }
  let blurred = sum / total;
  let brightness = params.color.w;

  if (params.flags.x < 0.5) {
    // Horizontal: blurprepare. Brightness above 1 lifts here; the default is
    // below 1, which is applied by the finish pass instead.
    let contrast = max(params.dir_radius.w, 0.0);
    let brighten = max(brightness, 1.0);
    var prepared = vec3<f32>(
      gain(blurred.r, contrast),
      gain(blurred.g, contrast),
      gain(blurred.b, contrast),
    ) * brighten;
    prepared = clamp(prepared, vec3<f32>(0.0), vec3<f32>(1.0));
    let vibrancy = params.color.x;
    if (vibrancy > 0.0) {
      prepared = apply_vibrancy(prepared, vibrancy, params.color.y, params.flags.y);
    }
    return vec4<f32>(prepared, 1.0);
  }

  // Vertical: blurFinish. Grain lands before the darkening multiply, exactly
  // like the CPU finish pass, then the channels are clamped to the valid range.
  let noise = params.color.z;
  let grain = (hash_noise(in.uv) - 0.5) * noise;
  var finished = blurred.rgb + vec3<f32>(grain);
  finished *= min(brightness, 1.0);
  return vec4<f32>(clamp(finished, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
