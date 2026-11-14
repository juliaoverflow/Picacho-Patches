# gpui-component

Upstream: [`gpui-component`](https://crates.io/crates/gpui-component) 0.5.1,
from [longbridge/gpui-component](https://github.com/longbridge/gpui-component).
License: Apache-2.0 (`LICENSE-APACHE`). Copyright belongs to Longbridge and the
gpui-component authors.

`crate/` is the crates.io release with `patches/` applied in order.

## Patches

### 0001 Window resize edges over the content

`resize_edge` in `src/window_border.rs`. With client-side decorations (Linux
and BSD), upstream found the resize edge under the pointer against
`window.window_bounds().get_bounds()`. While the window is maximized those are
its restore bounds, smaller than the surface, so everything below (and right
of) the restored size counted as an edge: the pointer showed a resize cursor
there and a click started a window resize instead of reaching the list,
sidebar or reader. Edges are now measured against `window.viewport_size()`,
and tiled or maximized sides, which have no shadow to grab, are never edges.

Remove when upstream measures edges against the drawn surface.

## Upgrading

```sh
scripts/vendor.sh rebuild gpui-component <version>
```

Then update the version above and in Picacho's `Cargo.toml`.
