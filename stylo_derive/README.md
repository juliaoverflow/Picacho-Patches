# stylo_derive

Upstream: [`stylo_derive`](https://crates.io/crates/stylo_derive) 0.22.0, from
[servo/stylo](https://github.com/servo/stylo).
License: MPL-2.0 (`LICENSE`; the source files carry their original headers).
Copyright belongs to The Servo Project Developers.

`crate/` is the crates.io release with `patches/` applied in order.

## Patches

### 0001 ToCss explicit error type

`to_css.rs`: generated `Ok(())` expressions explicitly name
`std::fmt::Error`. GPUI enables log's serde support, which adds
`From<serde_fmt::Error> for std::fmt::Error`; otherwise Stylo's generated
closures have ambiguous error types and an application using both does not
compile.

Remove when upstream's ToCss derive emits explicit error types.

## Upgrading

```sh
scripts/vendor.sh rebuild stylo_derive <version>
```

Then update the version above and in Picacho's `Cargo.toml`.
