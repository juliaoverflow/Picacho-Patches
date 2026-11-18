# gpui-mobile

Upstream: [itsbalamurali/gpui-mobile](https://github.com/itsbalamurali/gpui-mobile)
at commit `5fd54f4` (see `UPSTREAM`); its crates.io release only reserves
the name. GPUI's platform for Android (wgpu over Vulkan, or GL ES) and iOS.
License: Apache-2.0 (`LICENSE-APACHE`; upstream also offers GPL-3.0 and
AGPL-3.0, in `crate/`). Copyright belongs to Balamurali Pandranki and the
gpui-mobile authors.

`crate/` is that commit's `Cargo.toml`, `src`, licenses and README with
`patches/` applied in order. Picacho uses it on Android and iOS.

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

### 0002 iOS on gpui-pre 0.3.12

`Cargo.toml`, `src/ios/display.rs`, `src/ios/platform.rs`,
`src/ios/platform_view.rs` and `src/ios/window.rs`. 0001's changes for
iOS: the platform reports itself (`platform_info`: iOS, native), displays
report the screen's refresh interval (`maximumFramesPerSecond`),
display-change callbacks are accepted and never called, and the renderer's
GPU specs are optional. iOS now uses wgpu 30, the version gpui-pre-wgpu
0.3.12 builds its context from, and passes the surface configuration
`WgpuContext::new` now takes. The camera preview and video player views
reach their packages only with the `camera` and `video_player` features,
so the crate builds for iOS with default features off.

Remove when upstream builds against the gpui-pre release Picacho uses.

### 0003 Pinch

`src/pinch.rs` (new), `src/lib.rs`, `src/android/window.rs` and
`src/ios/window.rs`. GPUI's portable touch recognizer (gpui-pre 0.3.12) has
no pinch and ignores a second finger, so two fingers zoomed nothing. Raw
contacts now pass through `PinchRecognizer` on their way to it: one finger
goes on untouched; when a second lands the first is cancelled in GPUI, and
the two surface as `PinchEvent`s (`on_pinch`, as a trackpad's pinch on a
desktop) at their midpoint, each step's `delta` how much the distance
between them grew; lifting either ends it. Fingers left after a pinch, and
a third, are ignored until every finger has lifted.

Remove when GPUI's recognizer recognizes pinches.

## Upgrading

Change `commit` in `UPSTREAM`, then:

```sh
scripts/vendor.sh rebuild gpui-mobile
```
