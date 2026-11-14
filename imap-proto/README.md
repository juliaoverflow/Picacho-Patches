# imap-proto

Upstream: [`imap-proto`](https://crates.io/crates/imap-proto) 0.10.2, from
[djc/tokio-imap](https://github.com/djc/tokio-imap).
License: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Copyright belongs
to Dirkjan Ochtman and the tokio-imap contributors.

`crate/` is the crates.io release with `patches/` applied in order.

## Patches

### 0001 Gmail FETCH attributes

Parses Gmail's `X-GM-MSGID` and `X-GM-LABELS` FETCH attributes
(`src/parser/gmail.rs`, `AttributeValue::GmailMsgId` and
`AttributeValue::GmailLabels`). Without them, `imap` 2.4 rejects any response
that carries one, so Picacho could not use Gmail's permanent message ids or
show its labels.

### 0002 Unmodeled untagged data

Keeps untagged NAMESPACE, QUOTA, QUOTAROOT, MYRIGHTS, LISTRIGHTS, ACL, ID and
ENABLED data as `Response::Other` text (`resp_other` in
`src/parser/rfc3501.rs`). `imap` 2.4 otherwise takes any untagged response it
can't parse for the server hanging up.

### 0003 Building as a patched dependency

Two mechanical fixes that the registry build hides with `--cap-lints`: three
`src/macros.rs` arms drop a trailing semicolon in expression position, and
`src/lib.rs` allows warnings.

Remove these patches when Picacho moves to an `imap`/`imap-proto` release that
parses this data.

## Upgrading

```sh
scripts/vendor.sh rebuild imap-proto <version>
```

Then update the version above and in Picacho's `Cargo.toml`.
