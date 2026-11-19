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

### 0002 Acknowledge messages

Upstream never acknowledged what it received, so the server delivered every
message again at each login (each reconnect, each restart) until it expired,
and the same notification kept arriving. Now, as Chromium's MCS client does:

- each message with a persistent id is acknowledged as it arrives (a stream
  ack), and every stanza sent (heartbeats too) carries
  `last_stream_id_received`;
- the login names every received persistent id not yet confirmed (upstream
  sent only the last two), and once the server accepts the login they are
  settled and acknowledged by name (a selective ack);
- those ids are kept for the process by Android ID, so the client made for
  the next connection carries on; `persistent_ids`, `set_persistent_ids` and
  `on_persistent_ids` let a caller keep them across restarts;
- the server's heartbeat ping is answered with a heartbeat ack (upstream sent
  a ping back);
- reads time out every 30 seconds, so heartbeats go out on time, and a
  connection with no answer to a heartbeat within a minute is ended;
- a message that can't be read or decrypted is acknowledged and logged
  rather than ending the connection (which made it arrive again forever);
- redeliveries are dropped by persistent id from a bounded list, rather than
  one 28-day sleeping thread per message.

## Upgrading

Change `commit` in `UPSTREAM`, then run `scripts/vendor.sh rebuild fcm_receiver_rs`.
