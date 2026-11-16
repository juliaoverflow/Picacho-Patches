# gpui

Upstream: [`gpui`](https://crates.io/crates/gpui) 0.2.2, from
[zed-industries/zed](https://github.com/zed-industries/zed).
License: Apache-2.0 (`LICENSE-APACHE`). Copyright belongs to Zed Industries,
Inc.

`crate/` is the crates.io release with `patches/` applied in order.

## Patches

### 0001 Copying from a click on Wayland

`write_to_clipboard` and `write_to_primary` in
`src/platform/linux/wayland/client.rs`. Upstream set the selection with the
serial of the last key press. A copy made by clicking (a context menu's
"Copy Link Address") then carried an old serial, or 0 when no key had been
pressed, and compositors such as Mutter ignore a selection whose serial is
older than the current selection's: nothing was copied. The serial is now
that of the latest key press or mouse button (`SerialTracker::latest` in
`src/platform/linux/wayland/serial.rs`).

Remove when upstream uses the latest input serial for selections.

### 0002 Removed images give their atlas space back

`remove` in `src/platform/blade/blade_atlas.rs`,
`src/platform/mac/metal_atlas.rs` and
`src/platform/windows/directx_atlas.rs`. Upstream only counted a removed
tile out of its texture and freed the texture once nothing was left in it;
the tile's space was never returned to the texture's allocator. A texture
that also held images kept for good (avatars, emoji, icons) so lost the
space of every image ever dropped from it, and new images took new
textures: memory grew with every email Picacho's reader showed. The tile
is now deallocated. On Metal the key was also kept while its texture stayed,
so removing it again counted the tile out twice; it is now always removed.

Remove when upstream deallocates removed atlas tiles.

## Upgrading

```sh
scripts/vendor.sh rebuild gpui <version>
```

Then update the version above and in Picacho's `Cargo.toml`.
