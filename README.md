# openFSX

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="site/assets/brand/openfsx-dark.svg">
  <img src="site/assets/brand/openfsx-light.svg" alt="openFSX" width="480">
</picture>

An independent, experimental runtime for content from Microsoft Flight Simulator X, inspired by the approach of [openOMSI](https://github.com/openOMSI-org/openOMSI).

**Research prototype with airborne and runway starts.** Current work includes content inspectors, a textured MDLX viewer and keyboard-controlled flight using a limited CFG/AIR dynamics subset. Three fixed-wheel contacts support test-plane ground roll and departure. It is not a complete FSX replacement: reference-flight comparison, broader ground/landing behavior, scenery loading, gauges and add-on APIs remain ahead.

[Repository](https://github.com/openFSX-project/openFSX) · [Website](https://openfsx-project.github.io/openFSX/)

## Approach

Build a new engine in Rust, read content from the user's own FSX installation, and establish compatibility through reproducible tests. No original executable code, aircraft, textures, scenery or SDK binaries belong in this repository. The local `FSX/` directory is ignored by Git.

The viewer uses wgpu and winit, with Windows x64 as the first target and automatic graphics-backend selection. The first offscreen render was verified on an NVIDIA RTX 2080 through Vulkan. Full compatibility with existing aircraft and native add-ons is a long-term research goal, not an existing feature.

The compatibility target is FSX 1:1, including interface, flight behavior, content and supported APIs. This is an acceptance target, not a description of current coverage. Each subsystem needs reference comparisons before claiming parity.

## Quick start

Install stable Rust and the Windows MSVC C++ build tools. Run from the repository root:

```powershell
# Open the experimental FSX-style Free Flight menu (Windows / WebView2).
cargo run --features viewer --bin openfsx-viewer -- "./FSX" --menu

# List variants in your own FSX installation.
cargo run -- inspect "./FSX" --windows-1252

# Inspect geometry and available LODs.
cargo run -- inspect-model "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl"

# Open the experimental native viewer.
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1"

# Start an airborne C172 test with keyboard controls.
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --variant 0 --windows-1252 --lod 150

# Start on the original test runway, with parking brake applied.
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --ground --windows-1252 --lod 150
```

Drag to orbit, scroll to zoom, use arrow keys to rotate, press **R** to reset and **Esc** to close. The viewer displays bind-pose geometry with diffuse DDS textures and approximate alpha blending. DXT1/3/5 and masked 32-bit RGB DDS top mips are decoded; texture.cfg fallback directories are searched. Missing textures fall back to diffuse color and are reported. Visibility evaluation, animation playback and skin deformation remain absent. See [the viewer guide](docs/VIEWER.md).

The CFG inspector reads `title` values from `[fltsim.N]` under `SimObjects/Airplanes`. `--windows-1252` explicitly selects that code page for every file; omit it for ASCII/UTF-8 installations. Encodings are not guessed. The presence of `fsx.exe` checks directory structure, not license ownership. Neither tool launches FSX or modifies its installation.

Flight mode starts at 90 kt and 300 m with solved initial trim. Use **Down/Up** to pull/push, **Left/Right** to bank, **Q/E** for rudder, **Page Up/Down** for throttle, **Home/End** for trim, **P** to pause and **R** to reset. Add **--debug-hud** for the optional experimental on-screen panel. It reports true airspeed, height, vertical speed, local heading, throttle, trim, attitude and status. The ground is original test geometry. Read the [flight guide](docs/FLIGHT.md) for the actual data subset, approximation choices, controls and observed results. Flight behavior has not been validated against recorded FSX trajectories.

With --ground, **B** toggles the parking brake, **F4/F1** select full/idle power,
and **Space** holds wheel brakes. The same simulation continues through wheel
contact and liftoff. Run `cargo run -- takeoff-check "./FSX/SimObjects/Airplanes/C172" --windows-1252` for the documented parking, braking and departure checks.

The Windows **--menu** host reads the installed UI bitmap skin and aircraft thumbnails at runtime. Aircraft selection and **FLY NOW!** launch the experimental runway renderer; **Esc** returns to the menu. The catalog currently uses Windows-1252 explicitly. Other sections display their implementation status. This is an initial independent layout, not pixel-verified FSX parity. See [the viewer guide](docs/VIEWER.md).

## Website and GitHub Pages

The website includes [real engine screenshots](site/assets/screenshots/README.md)
of the textured C172 viewer and airborne prototype. Selected PNG captures are
published; source game resources remain excluded.

The original [logo and brand assets](site/assets/brand/README.md) include light,
dark and monochrome SVG wordmarks and a square GitHub avatar. The artwork and
its generator have a separate [MIT license](site/assets/brand/LICENSE).

The English website lives in **`site/`**. Its documentation-oriented layout follows the example of [openomsi.org](https://openomsi.org), with independent branding and content. Relative asset URLs and hash routes support the GitHub Pages `/openFSX/` path.

```powershell
node scripts/build-site.mjs
node scripts/preview.mjs
```

Open `http://127.0.0.1:4174`. The preview server exposes only public website files, never the adjacent game installation. Documentation is generated from `docs/` without npm or external Markdown scripts.

To publish: push to `main` and select **Settings → Pages → Build and deployment → Source → GitHub Actions**. The Pages workflow generates the documentation, uploads `site/`, then deploys it. Expected URL: `https://openfsx-project.github.io/openFSX/`. A manual trigger is also available. Only a successful Actions deployment confirms publication.

## Development

```powershell
cargo test --locked
cargo test --locked --features viewer
cargo fmt --check
cargo build --locked --features viewer --bin openfsx-viewer
node scripts/build-site.mjs --check
```

Tests use small, original synthetic data. Real installation checks are local and optional; game data is not redistributed with CI. Read [architecture](docs/ARCHITECTURE.md), [compatibility](docs/COMPATIBILITY.md), [format notes](docs/FORMATS.md) and [contribution guidance](CONTRIBUTING.md).

```text
src/                  CFG/AIR/MDLX/DDS readers, dynamics and native rendering
docs/                 English documentation, also published on the site
site/                 GitHub Pages website
scripts/              Documentation generation and local preview
.github/workflows/    Rust checks and Pages deployment
FSX/                  Local user installation — ignored, never distributed
target/               Builds and local snapshots — ignored
```

## Project status and rights

openFSX is independent and is not affiliated with Microsoft, Dovetail Games or openOMSI. Product names belong to their owners. Original content requires your own installation and compliance with applicable terms. Do not commit extracted models, textures, paid add-ons or SDK files.

The owner has not selected a source license yet. Public visibility alone does not grant an open-source license; add LICENSE before accepting external implementation contributions. No openOMSI or other third-party implementation code has been copied into this project.

## References

- [openOMSI](https://github.com/openOMSI-org/openOMSI): independent runtime approach.
- [Microsoft aircraft documentation](https://docs.flightsimulator.com/html/Content_Configuration/SimObjects/Aircraft_SimO/Aircraft.htm): modern documentation with legacy notes, not a full FSX specification.
- [FSDeveloper MDLX research](https://www.fsdeveloper.com/wiki/index.php/MDL_file_format_%28FSX%29): community observations, checked against local files and synthetic fixtures.
- [GitHub Pages custom workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
