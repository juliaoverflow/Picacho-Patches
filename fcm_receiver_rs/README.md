# fcm_receiver_rs

Upstream: [agusibrahim/fcm_receiver.rs](https://github.com/agusibrahim/fcm_receiver.rs)
0.1.1, at the commit in `UPSTREAM` (its `fcm_receiver_rs` folder). License: MIT
OR Apache-2.0, as its manifest declares; the repository ships no license file.

`crate/` is that commit with `patches/` applied in order. Picacho uses it,
through protected_email_bridge, to receive Proton push notifications.

## Patches

### 0001 Android installation registration

Android messaging tokens are requested the way the Firebase SDK requests them:
the Firebase installation is created first, then the token is registered with
the installation ID (`X-appid`) and its auth token. Installation IDs are 22
URL-safe characters, and the ID in the installation response is used when it
returns one. Web-push registration is unchanged. Saved tokens are not
rewritten; only new registrations change.

## Upgrading

Change `commit` in `UPSTREAM`, then run `scripts/vendor.sh rebuild fcm_receiver_rs`.
