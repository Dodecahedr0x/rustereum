# AGENT.md — instructions for agents working on rustereum

rustereum is an experimental Rust DSL for EVM contracts: `#[contract]` → IR →
Solidity → `solc` (via `foundry-compilers`) → bytecode, tested in `revm`. Read
[`README.md`](README.md) for the full overview before starting.

## Golden rules

1. **Run the full test suite after any change.** A feature isn't done until the
   whole workspace is green (see [Commands](#commands)). The OpenZeppelin
   examples must be `rustereum fetch`ed first, or their tests fail to resolve
   `@openzeppelin/...` imports.
2. **Keep the docs current.** Every new feature must be documented in **rustdoc**
   (`//!` module docs and `///` item docs) *and* in the relevant README. The
   rustdoc is published as a site, so it must always match reality and build
   clean under `RUSTDOCFLAGS="-D warnings"`. If you change the DSL surface, update
   the crate-level `//!` in `crates/rustereum-macros/src/lib.rs` and the
   language-subset table in `README.md`.
3. **No `unwrap`/`expect`/`panic!` in production code.** Library code (everything
   under `crates/*/src` that isn't a `#[cfg(test)]` block or a proc-macro
   `compile_error!` path) must return `Result` and propagate errors (e.g.
   `CompileError`). `unwrap`/`expect` are fine **only** in tests, examples, and
   the `testing` module's test harness. The proc macro reports user errors via
   `syn::Error`/`compile_error!`, never by panicking.
4. **Keep files small.** A source file should not exceed a few hundred lines. If
   it grows past that, split it into focused submodules (mirror the existing
   `ir` / `solidity` / `driver` / `vm` / `testing` split in `crates/rustereum`).

## Commands

```console
# Full test suite (revm end-to-end tests need the `testing` feature)
cargo nextest run --workspace --features rustereum/testing
cargo test -p rustereum --test ui          # trybuild macro compile-fail diagnostics

# The OZ examples need their Solidity deps fetched first (git-ignored lib/):
cargo build -p rustereum-cli
for d in examples/openzeppelin/*/; do (cd "$d" && cargo run -q -p rustereum-cli -- fetch); done

# CI gates — must pass:
cargo fmt --all --check
cargo clippy --workspace --all-targets --features rustereum/testing
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --features rustereum/testing
```

The first `solc`-backed test downloads a pinned `solc` via `svm` (needs network
once). CI is [`.github/workflows/ci.yml`](.github/workflows/ci.yml); mirror it.

## Where things live (touch-list for a language feature)

Adding a DSL feature usually means editing, in order:

1. `crates/rustereum/src/ir.rs` — the IR types the macro produces.
2. `crates/rustereum-macros/src/lib.rs` — parse the new syntax, lower to IR, emit
   `compile_error!` for anything unsupported.
3. `crates/rustereum/src/solidity.rs` — lower the new IR to Solidity.
4. `crates/rustereum/src/driver.rs` / `vm.rs` — only if compilation or the test
   client is affected.
5. **An example** under `examples/` exercising the feature end-to-end, plus unit
   tests, plus a `trybuild` case in `crates/rustereum/tests/ui/` for any new
   rejection.
6. **rustdoc + README** for the new surface.

The examples are the honest catalog of what the DSL can do — a feature without an
example that compiles + runs in `revm` is not shipped.

## Conventions & gotchas

- **`revm` is optional.** It lives behind the `rustereum/testing` feature. The
  `vm` module and the macro-generated client must compile **without** it; only
  the `testing` module (revm-backed) is feature-gated. Don't make `revm`
  mandatory.
- **Identifiers:** written `snake_case` in Rust, converted to `camelCase` in the
  emitted Solidity (so ABI selectors are idiomatic). Names that come *from*
  Solidity (parent contract names) are emitted verbatim.
- **No built-in macros in proc-macro output.** rust-analyzer can fail to expand
  built-in macros (`env!`, `include!`, `concat!`) inside generated code and then
  drops the whole expansion (hiding generated items). Resolve such values at
  macro-expansion time and bake them as literals instead (see how `compile()`
  bakes `CARGO_MANIFEST_DIR`).
- **rust-analyzer diagnostics are often stale** for proc-macro-heavy code
  ("proc-macro not yet built", false "unresolved import" for feature-gated
  modules). Trust `cargo build`/`cargo nextest`, not the editor squiggles.
- **OpenZeppelin sources are never committed.** Each example declares its deps in
  `rustereum.toml` (beside `Cargo.toml`); `rustereum fetch` clones them into a
  git-ignored `lib/` and generates `remappings.txt`. Keep `lib/` and
  `remappings.txt` out of git (already covered by `.gitignore`).
- **Commit messages** use a type prefix: `feat:`, `fix:`, `refactor:`, `docs:`,
  `test:`, `chore:`. Only commit/push when asked; branch off `master` for
  non-trivial work.
- **Verify behavior, don't assume.** After a change, run the actual example/test
  that exercises it (real `revm` execution), not just a type-check.

## Known limitations (don't silently work around them)

Types are `u256`/`Address`/`bool`; bodies are `self.field`, integer literals, and
`+`/`+=`/`=`; single inheritance; no function overrides, no interface
implementation/external calls, no internal calls with args. If a task needs one
of these, implement the feature properly (IR + macro + codegen + example + docs)
or report it as a gap — do not hack around it in a way that misrepresents what
the DSL supports.
