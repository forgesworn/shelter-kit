# shelter-kit

Shelter Kit is a Rust library, not a daemon: the shared core for applications
that need a secure, content-addressed Blossom store without coupling storage
policy to one network listener or product UI. It supplies an unbound Axum
router, BUD authorisation, streaming content-addressed storage, retention
policy, a transport-neutral fetch interface, and a verified mirror/repair
path. A product (for example a Wildbloom Node or Bothy) supplies the
listener, transport and consent model around it.

## Build & Test

| Command | Purpose |
|---------|---------|
| `cargo build` | Compile the library |
| `cargo test --locked` | Run the test suite |
| `cargo fmt --all -- --check` | Check formatting |
| `cargo clippy --all-targets -- -D warnings` | Lint, warnings deny |
| `cargo audit` | Dependency vulnerability audit |

CI (`.github/workflows/ci.yml`) runs the same fmt, clippy and test steps on
Linux, macOS and Windows, plus a separate `cargo audit` job. The pinned
toolchain is 1.94.1 (`rust-toolchain.toml`).

## Structure

```
src/
  lib.rs        - public re-exports
  blossom.rs    - AppState, BlossomConfig, router(), BUD routes
  store.rs      - Store: content-addressed storage, claims, retention, repair
  auth.rs       - AuthPolicy: BUD-01 event verification
  fetch.rs      - BlobFetcher trait, DirectHttpsFetcher, TorHttpFetcher
  admission.rs  - AdmissionFilter trait, SealedParcelsOnly
```

## Conventions

- British English in prose and comments.
- One TLS provider per process: `reqwest` is built provider-less; do not add
  a second rustls backend (see the comment in `Cargo.toml`).
- The crate is a library only: it never opens a port, runs Tor, encrypts
  application data or publishes Nostr events itself. Those are shell
  responsibilities.

## Key Files

| File | Purpose |
|------|---------|
| `CORE-CONTRACT.md` | The compatibility promise this crate implements |
| `CORE-CONTRACT-0.2.md`, `CORE-CONTRACT-0.3.md` | Joint contracts for earlier minor versions |
| `CHANGELOG.md` | Version history |
| `SECURITY.md` | Vulnerability reporting |

## Common Pitfalls

- `AppState::set_owner_keys`, `set_friend_grants` and `set_quota` each wait
  for a reconcile boundary (every write permit acquired) before swapping
  policy; do not assume the change is visible before that boundary crosses.
- A key cannot be an owner and hold a friend grant at once: promoting a
  friend to keeper is two ordered calls.
- Mirroring cannot reverse a tombstone; only a fresh, valid owner upload
  clears it.
- Schema 5 data (tombstones) must not be opened with a core older than
  0.4.0, including during a rollback. Schema 6 in 0.5.0 adds paid claims;
  older cores refuse it. Do not lower the schema version to bypass that check.
