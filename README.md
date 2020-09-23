# Lavalink-rs
A lavalink binder library for every Asynchronous discord crate.

### Links to download stuff you will need
- [Lavalink repository](https://github.com/Frederikam/Lavalink)
- [Java download](https://jdk.java.net/archive/) (11 or newer, 13 Recommended, 14+ Don't work)

### TODO
- [ ] Support multiple connections per region.
- [X] Support nodes.
- [X] Support pause, resume, skip to time.
- [X] Support starting at specific times and configurable replace current stream.
- [X] Support equalization.
- [ ] Support identifiers.
- [ ] Support both rustls and native_tls backends as features.
- [ ] Support twilight.
- [X] Support events.
- [ ] Support raw events.
- [ ] Implement my own event hander for voice connections.
- [X] Support easy queues natively.
- [X] Optimize the codebase.
- [X] Remove all the clones from examples.
- [X] Improve error handling.
- [ ] Add tracing and logging.
- [?] Add documentation.

### How to use

1: Install `openssl-dev` or `libssl-dev` (because native_tls_backend requires openssl in serenity)
- if the library is native and the bot is rustls, it works
- if the library is rustls and the bot is native, it works
- if the library and the bot are both native, it works
- but if the library and  the bot are rustl, it doesn't work

2: Install the version from crates.io:
```toml
lavalink-rs = "0.2.0-alpha"
```
Or the development release:

```toml
lavalink-rs = { git = "https://gitlab.com/nitsuga5124/lavalink-rs/", branch = "master" }
# or
[dependencies.lavalink-rs]
git = "https://gitlab.com/nitsuga5124/lavalink-rs/"
branch = "master"
```
