# blitz-paint

Upstream: [`blitz-paint`](https://crates.io/crates/blitz-paint) 0.3.0-beta.4,
from [DioxusLabs/blitz](https://github.com/DioxusLabs/blitz).
License: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Copyright belongs
to the Blitz authors.

`crate/` is the crates.io release with `patches/` applied in order.

## Patches

### 0001 Inline gradient backgrounds

`draw_inline_backgrounds` in `src/text.rs`, and the row and row-group colors
in `draw_table_row_backgrounds` (`src/render/background.rs`). Upstream paints
an inline box's `background-color` but not its `background-image`. Emails'
dark styles commonly set `background: linear-gradient(c, c)` on an element
whose light `background-color` comes from a later rule; browsers paint the
gradient over that color, so the element is dark. Without the gradient, a
`<span>` such as a tracking number showed white text on a white box. The patch
paints an inline box's gradient layers over its background color, and gives a
table row or row group whose top layer is a one-color gradient that color
(behind a cell's rounded corners, the light color showed as triangles).

Remove when upstream paints inline background images.

### 0002 Anonymous table rows

`draw_table_row_backgrounds` in `src/render/background.rs`. A block directly
in a table is laid out in an anonymous row of its own (blitz-dom's 0005),
whose `node_id` is the block itself. Its background is the block's, painted
as the cell's; as a row's it would be painted again behind it. Rows marked
`anonymous` paint no row background. Needs blitz-dom's 0005.

Remove with blitz-dom's 0005.

## Upgrading

```sh
scripts/vendor.sh rebuild blitz-paint <version>
```

Then update the version above and in Picacho's `Cargo.toml`.
