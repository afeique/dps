# Explosion Redesign — Volumetric Shaded-Sphere Strategy (design only)

**Status:** STRATEGY ONLY — nothing here is implemented. The old flat-circle
`render::fireball` system was removed (commit "Remove the flat-circle fireball
explosion layers"); this document is the plan for its replacement.

---

## 1. Why the old explosions looked flat (root cause, confirmed)

Not a hardware or renderer limitation — the renderer already does HDR + bloom +
alpha blending correctly. The flatness was three implementation choices:

1. **Uniform vertex color.** Each "blob" was a triangle-fan mesh where *every*
   vertex (incl. the center) got the same RGBA `[1,1,1,1]`. So each blob was a
   solid, uniformly-filled polygon — a paper cut-out, not a sphere. A real puff
   is opaque/bright in the middle and fades to transparent at the rim (a radial
   gradient). There was none.
2. **Genuinely 2D, no depth cue.** `Mesh2d` discs on a flat z-plane, no normals,
   no per-fragment shading. The only way to fake volume on a 2D billboard is the
   gradient/normal math in (1) and a fragment shader — neither was present.
3. **Plain alpha blend, not additive.** Overlapping translucent discs just sat
   on top of each other instead of *accumulating* brightness, so the hot core
   never built up the glow real fire has.

The fix is to stop drawing flat filled circles and instead draw **shaded sphere
impostors** (billboards whose fragment shader makes them read as 3D translucent
spheres), composited into a cloud.

---

## 2. Research — techniques surveyed

### A. Spherical billboards / "soft particles" (Umenhoffer & Szirmay-Kalos)
A billboard quad, but the **fragment shader treats it as a slice through a
sphere**: for each fragment it computes how far a view ray travels *through* the
virtual sphere (entry→exit thickness), and opacity follows Beer–Lambert:

```
thickness = min(sphere_exit, scene_depth) − max(sphere_entry, near)
opacity    = 1 − exp(−density · thickness)
```

Thickness is max at the sphere center and → 0 at the silhouette, giving a
**soft, naturally-radial** edge with no hard rim and no clipping/popping against
geometry. This is the canonical real-time volumetric-explosion primitive (the
paper renders fire+smoke at 70 FPS with ~16/135/112 particles). It is the single
biggest win for "not flat," and it is cheap.

### B. Impostor sphere shading (fake-3D billboard)
In the fragment shader, treat the quad's local UV as a unit disc `p = uv*2−1`;
outside `|p|>1` discard. Reconstruct a hemisphere normal `n = (p.x, p.y,
sqrt(1−p·p))`. Then shade with a fixed key light `L`: `lambert = max(dot(n,L),0)`,
add a **fresnel/rim** term `pow(1−n.z, k)` for the glowing edge, and a
center-hot falloff. Result: each particle looks like a lit, rounded sphere
instead of a flat disc — depth for ~free, no raymarching.

### C. Volumetric raymarching of an SDF-sphere cloud (the "beautiful" tier)
Combine several sphere SDFs with a **smooth-min (`smin`)** so they melt into one
organic blob; displace the surface/density with **FBM noise**; raymarch the
volume accumulating density; shade with Beer's-law light absorption + a secondary
march toward the light for self-shadowing:

```
for step along view ray:
    d = density(smin(spheres) + fbm(p, t))
    light = exp(−shadowDensityTowardLight · absorb)
    color += d · light · rampColor · transmittance
    transmittance *= exp(−d · absorb · stepSize)
```

Most realistic (true self-shadowed billowing smoke), most expensive. Overkill for
many small 2D pops but gorgeous for bosses.

### D. Procedural turbulence to drive motion
- **FBM** (sum of noise octaves) for cloud density detail.
- **Domain warping** (feed FBM into its own input) for swirling, organic shapes.
- **Curl noise** (curl of a noise field) gives a *divergence-free* velocity field
  → smoke that advects/rolls like fluid, cheaply, for animating either particle
  positions or the shader's sample coordinates over the explosion's life.

### E. Color: blackbody-style ramp
Map a 0→1 heat scalar `c` to fire color with diverging power curves so the core
is white-hot and the edge deep-red, e.g. `vec3(1.5·c, 1.5·c³, c⁶)`. Drive `c`
down over life (cooling) and by radius (center hotter). Keep the per-element
palette by tinting the ramp (Pyro=orange, Cryo=blue-white, Volt=violet, …).

### F. Offline-baked flipbooks (EmberGen, asset route)
Sim a real fluid explosion offline, bake to a sprite-sheet/VDB, sample at
runtime. Best fidelity-per-frame-cost and AAA-standard — but it's a **texture
asset pipeline**, against this project's grain (everything is procedural, no
sprite assets, lyon/hanabi only). Noted for completeness; not recommended here.

---

## 3. Recommended approach for `dps`

**Tier-1 core: a procedural cloud of shaded sphere impostors (B) with soft-
particle opacity (A), additive hot core + alpha-blended smoke, blackbody-tinted
per element, that expands as it fades fast.** Optionally a Tier-2 raymarched (C)
variant reserved for bosses later.

Rationale: it's a 2D game with HDR+bloom already; impostor + soft-particle gets
~all the volumetric read for a fraction of raymarching's cost, needs no texture
assets (stays procedural), and slots straight into the existing `Death`-message
FX pipeline. Bevy 0.18 supports this via a custom `Material2d` (`AsBindGroup` +
a small WGSL fragment shader) on a unit-quad `Mesh2d` — the codebase has **no
custom shaders yet**, so this introduces the first one (a deliberate, contained
step).

### 3.1 Anatomy of one explosion (procgen recipe)
A blast = a short-lived **cloud of N billboard quads** (N ≈ 6–12 scaled by enemy
size / boss tier), each a sphere impostor, spawned at the death point with:
- **Placement:** offsets sampled in a small disc (golden-angle or hash) so the
  cloud is lumpy and asymmetric, not concentric. A couple of big slow spheres +
  several small fast ones (size variance is what sells scale).
- **Two populations, two blend modes:**
  - **Fire core** (inner, few, additive): blackbody ramp hot→warm, *sustain*
    then `pow(0.7)` fade (rainboids' core curve), brightest — only this blooms.
  - **Smoke** (outer, more, alpha-blend): dim element-tinted gray, `pow(0.45)`
    slow fade, lingers and drifts out; absorbs/occludes so the cloud has form.
- **Expand-as-fade (fast):** each sphere grows from ~0.3→1.0 radius on a cubic-
  out curve while its opacity falls — the user's "expands as it fades out
  quickly." Lifetimes short and staggered (~0.3–0.9 s; core shorter than smoke)
  so it reads as a punchy burst, not a lingering balloon.
- **Motion:** outward drift + drag (existing `Velocity`/`integrate`), plus the
  shader animates its noise sample coords by **curl/FBM over life** so each
  sphere's surface roils instead of being a static gradient. A little spin per
  sphere too.
- **Shading per fragment (the WGSL):** disc-mask + hemisphere normal (B) → key-
  light lambert + fresnel rim + center-hot; multiply by soft-particle thickness
  opacity (A) for the soft edge; sample FBM for surface mottling; map heat→color
  via the blackbody ramp tinted to the kill's element; output premultiplied for
  the chosen blend.
- **Palette discipline:** one element drives the whole blast (a `heat→color`
  ramp parameterized by the element hue), so every sphere in a blast shares a
  family but different enemies explode in different colors.

### 3.2 Supporting layers (keep / reuse)
- **Wavefront rings** (`reaction_fx::spawn_death_rings`): keep — thin expanding
  *circular rings* are the pressure front and were explicitly wanted. Possibly
  upgrade to a shader ring with a soft radial falloff later.
- **Sparks / debris / embers** (`explosion` hanabi + `asteroid_debris`): keep as
  the particle layer that flies across screen.
- **Screen shake + flash + hitstop:** keep — impact, not shape.

### 3.3 Lifecycle / integration
- A `spawn_explosion` system reads `Death` (as the old one did) and spawns the
  quad cloud with a new `ExplosionSphere` component holding per-sphere params.
- A `tick_explosions` system advances life → updates each sphere's Transform
  scale (grow) + drift + feeds `life`, `heat`, `seed` into its material uniforms
  (alpha fade, ramp, noise time). Despawn via shared `Lifetime`/`tick_lifetimes`.
- Material: one `Material2d` (`ExplosionMaterial`) with uniforms `{ life, heat,
  density, element_color, seed, mode }`; instances share the shader, vary by
  uniform. (Avoid per-frame `ColorMaterial` alpha churn — that was the old hack.)

---

## 4. Cost, risks, scope

- **Perf:** N≈10 alpha quads per kill with a light fragment shader is cheap;
  soft-particle thickness needs the depth texture (or an analytic z since these
  are 2D with no intersecting geometry — can simplify to the impostor falloff
  alone, skipping the depth read). Cap concurrent explosions; cap N on bosses.
- **Transparency sort:** many additive+alpha quads overlapping → rely on additive
  (order-independent) for the core; keep smoke counts modest. Bevy 2D draws
  transparent back-to-front by z; give the cloud a tight z-band.
- **First custom shader in the repo:** small but new infra (WGSL + `AsBindGroup`
  + plugin registration). Contained to the explosion module.
- **Raymarched (C) tier:** defer; only worth it for big boss deaths.

## 5. Phased implementation (when greenlit — not now)
1. **Impostor sphere `Material2d` + WGSL** (disc mask, hemisphere normal, lambert
   + fresnel + center-hot, blackbody ramp, life-driven alpha). One quad test.
2. **Cloud spawner/ticker** off `Death` (N spheres, fire-core additive + smoke
   alpha, expand-as-fade, drift, per-element palette). Replaces old fireball.
3. **FBM/curl surface animation** in the shader (roiling, domain-warped).
4. **Tune** sizes/lifetimes/counts vs. screenshots; boss scaling.
5. (Optional) **Raymarched boss explosion** variant.

Each phase: build clean, keep tests green, verify with a demo screenshot.

---

## Sources
- Umenhoffer, Szirmay-Kalos — *Spherical Billboards and their Application to
  Rendering Explosions* (soft particles / Beer–Lambert thickness opacity):
  http://cg.iit.bme.hu/~szirmay/explosionShaderX.pdf
- Maxime Heckel — *Painting with Math: A Gentle Study of Raymarching* (SDF
  smin union + FBM density + Beer's-law volume shading):
  https://blog.maximeheckel.com/posts/painting-with-math-a-gentle-study-of-raymarching/
- *Stylized Fireball Shader* (layered noise + domain warping + color ramp on a
  sphere): https://as7tesia.com/projects/fireball-shader/
- Bridson — *Curl-Noise for Procedural Fluid Flow* (divergence-free advection):
  https://www.cs.ubc.ca/~rbridson/docs/bridson-siggraph2007-curlnoise.pdf
- JangaFX EmberGen (offline-baked flipbook alternative):
  https://jangafx.com/software/embergen
