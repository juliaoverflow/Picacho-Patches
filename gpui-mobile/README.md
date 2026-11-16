# gpui-mobile

Upstream: [itsbalamurali/gpui-mobile](https://github.com/itsbalamurali/gpui-mobile)
at commit `5fd54f4` (see `UPSTREAM`); its crates.io release only reserves
the name. GPUI's platform for Android (wgpu over Vulkan, or GL ES) and iOS.
License: Apache-2.0 (`LICENSE-APACHE`; upstream also offers GPL-3.0 and
AGPL-3.0, in `crate/`). Copyright belongs to Balamurali Pandranki and the
gpui-mobile authors.

`crate/` is that commit's `Cargo.toml`, `src`, licenses and README with
`patches/` applied in order. Picacho uses it on Android only.

## Patches

### 0001 gpui-pre 0.3.12

`Cargo.toml`, `src/android/display.rs`, `src/android/platform.rs` and
`src/android/window.rs`. Upstream builds against gpui-pre 0.3.6; Picacho
uses 0.3.12, the release gpui-component 0.9.1 pins. The platform now
reports itself (`platform_info`: Android, native), displays report no
refresh interval (frames follow Choreographer), display-change callbacks
are accepted and never called (an activity keeps its display), frame
requests fill the new timing fields with their defaults, and the
renderer's GPU specs are optional.

Remove when upstream builds against the gpui-pre release Picacho uses.

## Upgrading

Change `commit` in `UPSTREAM`, then:

```sh
scripts/vendor.sh rebuild gpui-mobile
```
