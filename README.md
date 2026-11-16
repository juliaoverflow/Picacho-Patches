# Picacho Patches

Third-party crates that [Picacho](https://github.com/pinaltech/Picacho) uses
with small patches, pulled in through Cargo's `[patch.crates-io]`.

| Software | Patches |
| --- | --- |
| [blitz-dom](blitz-dom) 0.3.0-beta.4 | Unresolvable URLs don't panic; tables restacked for narrow screens; legacy table attributes |
| [blitz-paint](blitz-paint) 0.3.0-beta.4 | Inline gradient backgrounds |
| [gpui](gpui) 0.2.2 | Copying from a click on Wayland; removed images give their atlas space back |
| [gpui-component](gpui-component) 0.5.1 | Window resize edges over the content |
| [imap-proto](imap-proto) 0.10.2 | Gmail FETCH attributes; unmodeled untagged data; builds as a patched dependency; extension data (thread/email ids, PREVIEW, SAVEDATE, BINARY, ESEARCH, SORT, THREAD, VANISHED) |
| [stylo_derive](stylo_derive) 0.22.0 | ToCss explicit error type |

Each folder has the crate (`crate/`), its patches, its license and a README.

All rights to this software belong to its original authors. Each crate keeps
its upstream license, found in its folder; the patches are offered under the
same license.
