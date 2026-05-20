# bevy_prototype_lyon vs bevy_vello — Silhouette Renderer Evaluation

**Date:** 2026-05-20  
**Author:** Afeique Sheikh  
**Resolves:** `docs/port-plan.md` §3.1 — open item "Evaluate vello vs lyon"  
**Scope:** Vector-silhouette rendering only (ship + enemy shapes from `shapes.js`).  
**Decision:** Stay on `bevy_prototype_lyon`.

---

## 1. Context and the Non-Negotiable Requirement

Dark Prism Solid reproduces ~446 authored Canvas2D path calls
(`moveTo`/`lineTo`/`arc`/`bezier`/`fill`/`stroke`) as runtime silhouettes.
Glow is **not** per-shape blur; it is the interaction of two pipeline stages:

1. **Over-bright emissive vertex colors** — linear-RGB components well above 1.0
   (e.g. `Color::linear_rgb(0.0, 6.0, 8.0)` for the Drifter's electric cyan
   edge, `Color::linear_rgb(8.0, 9.0, 9.0)` for the white-hot core).
2. **Bevy's `Bloom` post-process** — reads the HDR framebuffer, finds values
   above the luminance threshold, and writes a glow halo back into the image
   before tonemapping.

For `Bloom` to see those values, they must survive untouched into the HDR
view target that the camera owns — typically an `Rgba16Float` texture when
`Hdr` is active. Any intermediate stage that clamps or re-encodes colors into
8-bit SDR before they reach that target **silently kills the glow**, reducing
emissive cyan to flat white or clipping it to `[0, 1]`.

This is the single most important question when evaluating an alternative
renderer: **does it write HDR-linear values into the camera's HDR view, or
does it route through its own SDR intermediate?**

---

## 2. How Each Crate Works

### 2.1 `bevy_prototype_lyon` 0.16.0

**Architecture — direct `Mesh2d` / `ColorMaterial` path.**

`ShapePlugin` runs `mesh_shapes_system` in `PostUpdate`. For each `Shape`
entity it tessellates the lyon path into a triangle list and stores the result
as a `Mesh2d`. Per-vertex colors are written to `Mesh::ATTRIBUTE_COLOR` as
raw `[f32; 4]` linear values via `color.to_linear().to_f32_array()` — no
clamping, no 8-bit quantisation. The entity carries
`MeshMaterial2d<ColorMaterial>` with a shared white material; the vertex
color attribute is multiplied by that in the `ColorMaterial` shader, which is
Bevy's own 2D mesh shader.

Critically, Bevy's `ColorMaterial` / `Mesh2dPipeline` **writes into whatever
render target the camera owns**. When that camera has `Hdr` enabled, the
view target is `TextureFormat::Rgba16Float` (or similar HDR format). The
shape mesh is just another 2D entity rendered in the same `Opaque2d` /
`Transparent2d` phase as sprites. There is no intermediate texture, no
separate pass, and no format conversion — the `f32` vertex colors land
verbatim in the HDR framebuffer. Bloom sees exactly what was authored.

**Source evidence:**
- `src/vertex.rs`: `color: self.color.to_linear().to_f32_array()` — raw
  linear `f32` components stored per vertex.
- `src/plugin.rs` → `build_mesh()`: inserts `Mesh::ATTRIBUTE_COLOR` as
  `Vec<[f32; 4]>` and builds a `Mesh2d` with `PrimitiveTopology::TriangleList`.
- `src/entity.rs`: `#[require(Mesh2d, MeshMaterial2d<ColorMaterial>, …)]` —
  the shape is a plain Bevy 2D mesh entity; no custom render graph node.

### 2.2 `bevy_vello` 0.13.1

**Architecture — SDR intermediate texture + fullscreen compositing quad.**

Vello's GPU-compute renderer (`vello::Renderer`) requires its render target
to be `TextureFormat::Rgba8Unorm` with `STORAGE_BINDING` — this is a **hard
requirement** stated in the vello 0.7 `RenderParams` documentation:

> "The texture must be created with the `Rgba8Unorm` format and the
> `TextureUsages::STORAGE_BINDING` flag set."

`bevy_vello` honours this exactly. `setup_image()` in
`src/render/systems.rs` creates:

```rust
format: TextureFormat::Rgba8Unorm,
usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST
     | TextureUsages::STORAGE_BINDING,
```

Every frame, vello renders the entire scene into that 8-bit SDR texture.
`setup_rendertarget()` then spawns a fullscreen quad `Mesh2d` carrying
`MeshMaterial2d<VelloCanvasMaterial>`, which samples the SDR texture through
the shader at `shaders/vello_rendertarget.wgsl`. That shader:

1. Samples the `Rgba8Unorm` texture.
2. Applies a manual sRGB-to-linear conversion
   (`linear_from_srgba()`).
3. Outputs the result — which is now linear `[0, 1]` — to the camera's HDR
   target via `BlendState::ALPHA_BLENDING`.

The consequence is irreversible: **all color information was quantised to 8
bits per channel and clamped to `[0, 255]` before the vello compute kernel
ran.** Any linear-RGB component authored above 1.0 was silently clamped to
1.0 (255/255) inside vello's peniko color model, which operates on normalised
`u8` or `f32` values in `[0, 1]`. By the time the compositing quad draws into
the HDR framebuffer, every emissive value is already flat white (`[1,1,1,1]`).
Bloom never sees values above its threshold; there is no glow.

The `FIXME` comment in `VelloCanvasMaterial::specialize()` confirms the team
is aware of compositing edge cases:

```rust
// FIXME: Vello isn't obeying transparency on render_to_surface call.
// See https://github.com/linebender/vello/issues/549
```

There is no open issue or PR in the `linebender/bevy_vello` repository
addressing HDR render target support, and the upstream vello project has no
planned `Rgba16Float` path as of 2026-05-20.

---

## 3. Comparison Table

| Criterion | `bevy_prototype_lyon` 0.16 | `bevy_vello` 0.13.1 |
|---|---|---|
| **HDR / Bloom passthrough** | **Yes.** Vertex colors are raw `f32` linear values written directly into the camera's HDR target (`Rgba16Float`). Emissive `> 1.0` values survive to Bloom intact. | **No.** All content is rasterised into `Rgba8Unorm` (8-bit SDR, clamped to `[0,1]`). Values above 1.0 are destroyed before the compositing quad reaches the HDR pass. Bloom sees only `[0,1]` whites. |
| **Render integration** | Plain `Mesh2d` entity. Rendered in Bevy's standard `Transparent2d` / `Opaque2d` phase. No custom render graph nodes. | Custom `VelloRenderPlugin` with its own `RenderSystems::Render` system. Renders to an intermediate texture, then composites via a fullscreen `Mesh2d` quad. |
| **Z-ordering with other 2D entities** | Standard Bevy z-ordering via `Transform::translation.z`. Works identically to sprites and `bevy_hanabi` particles. | Z-ordering only applies to the compositing quad's z-value; **all vello content is baked into one flat texture per frame** and composited as a single layer. Fine-grained z-interleaving with sprites or particles is not supported. |
| **Path fidelity** | Triangle tessellation. Straight lines and fills are exact. Curves (beziers, arcs) are approximated by tessellation — quality scales with segment count. No anti-aliasing at the path level (relies on MSAA). | GPU-compute area-antialiased rasterisation. Sub-pixel accuracy, smooth beziers and arcs at any zoom level, no triangle popping on thin strokes. Analytically superior quality per path. |
| **Anti-aliasing** | Per-shape MSAA (`Msaa::Sample4`). Adequate for game silhouettes; diagonal strokes show mild aliasing at low sample counts. | Analytic area AA. Stroke edges and curve boundaries are mathematically smooth at the pixel level. |
| **Stroke width fidelity** | Tessellated stroke mesh — constant width in model space, scales with the mesh transform. Thin strokes (< 1 px screen) can vanish. | Screen-space aware; strokes remain visually consistent at sub-pixel widths. |
| **Porting Canvas2D paths** | Natural: `ShapePath` mirrors `moveTo` / `lineTo`; lyon has direct `cubic_bezier_to`, `arc`, and `close` calls. Already proven in `src/render/shapes.rs`. | Kurbo path API (`BezPath`, `kurbo::Arc`) is similarly expressive; requires translating to vello `Scene::fill` / `stroke`. Slightly more indirect — must build a `Scene` each frame rather than updating ECS component data. |
| **Per-shape color changes** | Update `Shape.fill` / `Shape.stroke` ECS components; `mesh_shapes_system` retessellates on `Changed<Shape>`. Per-entity granularity; no frame-wide re-encode. | Entire scene is re-encoded to the vello scene buffer every frame in `render_frame`. Color changes require rebuilding scene command buffers. |
| **Performance (hundreds of paths)** | CPU tessellation in `PostUpdate` only on change; GPU draws static triangle meshes per entity. Scales well; lyon's triangle submission overhead is low. | GPU-compute rasteriser scales very well for complex scenes (handles `paris-30k` at 177 fps). For a few hundred simple game silhouettes the CPU scene-encode overhead is low, but the full-screen texture blit is an additional cost every frame regardless of path count. |
| **Bevy 0.18 compatibility** | Yes. v0.16.0 released 2026-01, explicitly targets `bevy ^0.18.0`. | Yes. v0.13.1 updated to Bevy 0.18 + vello 0.7 in Jan 2026 (PR #198). |
| **GPU requirement** | Standard wgpu rasteriser, no compute shaders required. | **Requires compute shader support.** Fails to initialize on GPUs without compute (rare on desktop, but a constraint). |
| **Maturity** | In production use; the API has been stable across several Bevy releases; 811 stars, 105 forks, 24 open issues, actively maintained. | Actively maintained by Linebender. Upstream vello is explicitly "alpha". The bevy integration tracks Bevy releases reliably but the rendering core has known gaps (blur/filter effects, conflation artifacts, glyph caching). |
| **API churn risk** | Low. Tracks Bevy minor releases; one maintainer bump per Bevy release cycle. | Moderate. Upstream vello API evolves; bevy_vello must track both Bevy and vello versions simultaneously. |

---

## 4. Key Trade-offs in Prose

### The HDR-bloom gap is architectural, not a configuration option

Vello's `Rgba8Unorm` render target is not a conservative default that can be
overridden by passing an HDR format to `setup_image`. It is a **hard
constraint of the vello GPU compute pipeline**: `vello::Renderer::render_to_texture`
requires the texture to have `STORAGE_BINDING` usage, which `Rgba16Float`
textures do support, but the vello compute shaders write 8-bit packed color
data internally. The `RenderParams` docs state `Rgba8Unorm` is the required
format; `Rgba8UnormSrgb` "might" work but is untested; `Rgba16Float` is not
listed. This is not a limitation of `bevy_vello` specifically — it is a
limitation of upstream vello as of 0.7. Circumventing it would require either
patching vello's compute shaders or maintaining a fork, both of which are
untenable dependencies for a solo game project.

### Path quality: vello wins — but the margin matters less in practice

Vello's analytic anti-aliasing is genuinely superior to lyon's triangle
tessellation for fine strokes and tight curves. For the authored silhouettes
in `shapes.js` — broad fills with 1–3 px emissive strokes rendered at
typical game resolution — the quality difference is perceptible under a
magnifying glass but not in motion. The bloom halo that the HDR pipeline adds
around each shape naturally blurs edge sharpness, making sub-pixel accuracy
less impactful visually. If the glow pipeline worked in vello, this would be a
secondary advantage worth weighing; since it does not, it is moot.

### Z-interleaving with `bevy_hanabi`

The particle system (`bevy_hanabi`) writes directly into the main pass as
`Mesh2d`-based quads at specific z-depths. Lyon shapes do the same. This
means a player death explosion can appear both in front of and behind ship
hull components by simply setting z-values, with zero extra coordination.
With vello, all vello content is flattened into one SDR texture and composited
at a single z-depth: everything vello draws is either entirely behind, or
entirely in front of, every `bevy_hanabi` particle. Reproducing the web
build's layering (background dark hull fill, glow stroke on top of hull, core
on top of stroke, some particles above the ship, some below) would require
splitting the scene into multiple vello canvases at different z-depths — a
significant architectural complication.

### API ergonomics for the port

`bevy_prototype_lyon` maps almost 1:1 to Canvas2D idioms: `ShapePath::move_to`
/ `line_to` / `arc` / `cubic_bezier_to` / `close` mirror the Canvas2D
primitives in `shapes.js`. The existing `src/render/shapes.rs` already proves
this with `ship_hull()`, `ship_cockpit()`, `drifter_star()`, and
`drifter_core()` — all ported verbatim from the JS with only a Y-axis flip.
Vello's `kurbo::BezPath` is similarly expressive but requires encoding into a
`Scene` each frame rather than updating ECS components, which fights the
"change-only" optimisation that lyon's `Changed<Shape>` detection provides.

---

## 5. Recommendation

**Stay on `bevy_prototype_lyon`.** Do not migrate to `bevy_vello` for gameplay
silhouettes.

The deciding factor is architectural and non-negotiable: vello rasterises into
`TextureFormat::Rgba8Unorm`, which irreversibly clamps all color components to
`[0, 1]` before the compositing quad reaches the camera's HDR framebuffer.
Emissive colors authored at `linear_rgb(0.0, 6.0, 8.0)` arrive at Bloom as
`(0, 1, 1)` — flat cyan, no overbrightness, no glow. The entire HDR-bloom
glow pipeline that justifies the native port's visual ambition breaks silently.

Lyon tessellation into `Mesh2d` is the correct tool here precisely because it
is *not* a vector-rendering abstraction — it is a mesh generator that speaks
the same language as the rest of Bevy's 2D pipeline. Emissive vertex colors
are raw `f32` data that flow from the CPU into the HDR framebuffer without any
intermediate that could touch them.

### Hybrid path for the future

This recommendation does not preclude ever using `bevy_vello`. Two scenarios
where it could add value without touching the glow pipeline:

1. **Complex UI vector art** (title screen, menu decorations, SVG logos) where
   crisp analytic curves matter and HDR emissive is irrelevant.  A separate
   vello canvas on a UI render layer would work cleanly.
2. **Static background art** (non-emissive nebula layers, static decorative
   overlays) where the SDR limitation is not a constraint.

In both cases, `bevy_vello` would run on a layer that never interacts with the
bloom pass, keeping the emissive game entities entirely in lyon / `Mesh2d`.

---

## 6. Sources

| Source | URL / reference |
|---|---|
| `bevy_prototype_lyon` 0.16.0 crate | https://crates.io/crates/bevy_prototype_lyon/0.16.0 |
| `bevy_prototype_lyon` source — `vertex.rs` | https://github.com/Nilirad/bevy_prototype_lyon (via `gh api`) |
| `bevy_prototype_lyon` source — `plugin.rs` | https://github.com/Nilirad/bevy_prototype_lyon (via `gh api`) |
| `bevy_prototype_lyon` source — `entity.rs` | https://github.com/Nilirad/bevy_prototype_lyon (via `gh api`) |
| `bevy_vello` 0.13.1 crate | https://crates.io/crates/bevy_vello/0.13.1 |
| `bevy_vello` source — `src/render/systems.rs` | https://github.com/linebender/bevy_vello (via `gh api`) |
| `bevy_vello` source — `src/render/mod.rs` | https://github.com/linebender/bevy_vello (via `gh api`) |
| `bevy_vello` shader — `shaders/vello_rendertarget.wgsl` | https://github.com/linebender/bevy_vello (via WebFetch) |
| vello `RenderParams` docs | https://docs.rs/vello/latest/vello/struct.RenderParams.html |
| vello repository / alpha status | https://github.com/linebender/vello |
| Bevy 0.18 migration guide | https://bevy.org/learn/migration-guides/0-17-to-0-18/ |
| `bevy_hanabi` 0.18.0 | https://crates.io/crates/bevy_hanabi/0.18.0 |
