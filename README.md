# Shelter Kit

Shelter Kit is the shared Rust core for applications that need a secure,
content-addressed Blossom store without coupling storage policy to one network
listener or product UI.

It is deliberately a library, not a daemon.  A product supplies the listener,
transport and user experience; Shelter Kit supplies the unbound Axum router,
BUD authorisation, streaming content-addressed store, retention policy,
transport-neutral fetch interface and verified mirror/repair path.

Wildbloom and Bothy remain separate products because publishing and private
shelter have different consent and failure modes.  They can use the same core:
a Wildbloom Node is an owner-first keeper using Tor or direct HTTPS, while Bothy
can add its own pairing, federation and native transport around the same store
and router.

## Implemented in 0.1.1

- BUD-01, BUD-02, BUD-04, BUD-06, BUD-11 and self-only BUD-12 routing.
- Streaming reserve, hash verification and atomic content-addressed storage.
- One deduplicated store with owner, friend and best-effort guest claims.
- Fixed watermarks, expiring friend grants and oldest-guest-first eviction.
- Opaque serving for friend-only and guest-only blobs.
- A `BlobFetcher` boundary that keeps transport separate from authorisation,
  retention, hashing and storage.
- Direct public-HTTPS and loopback-proxied Tor fetch adapters, both with
  redirects disabled.  Direct DNS refuses non-public address space.
- Verified mirror sources and exact-length, exact-hash repair.
- SQLite schema migration and interrupted-transaction reconciliation.

The exact compatibility promise is in [CORE-CONTRACT.md](CORE-CONTRACT.md).

## Not implemented here

Shelter Kit does not open a port, run Tor, punch through NAT, provide STUN/TURN,
encrypt application data, publish Nostr identity/status events, discover peers,
promise a replica count or prove durable custody.  Those are shell, application
or future protocol responsibilities.  Transport success is evidence that bytes
moved, not evidence that another machine will retain them.

## Use

```toml
[dependencies]
shelter-kit = { git = "https://github.com/forgesworn/shelter-kit", tag = "v0.1.1" }
```

Create a `Store`, build an `AppState` from a `BlossomConfig`, optionally supply
a `BlobFetcher`, then mount `shelter_kit::router(state)` into the product's
listener.  The config carries the shell's public product name and HTTPS source
URL, so shared storage does not blur the Wildbloom/Bothy front doors.  Tor and
direct public HTTPS are supplied fetch adapters; an application may provide
another adapter without moving trust decisions into that transport.

## Status

This is a production candidate extracted from the reviewed Wildbloom Node core.
It has adversarial local tests and CI for macOS, Linux and Windows, but has not
received an independent security audit.  Do not treat it as a backup or custody
promise without independent replicas, monitoring and recovery evidence.

MIT licensed.  Sponsorship links are in [.github/FUNDING.yml](.github/FUNDING.yml).
