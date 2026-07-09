# rustereum-cli

The `rustereum` command-line tool: scaffold projects and manage Solidity
dependencies. Solidity sources are **not committed** — they're declared in a
`rustereum.toml` manifest and fetched into a git-ignored `lib/`.

```console
$ cargo build -p rustereum-cli      # produces target/debug/rustereum
```

## Commands

### `rustereum new <name>`

Scaffold a new project in `./<name>`:

```
<name>/
├── Cargo.toml
├── rustereum.toml        # dependency manifest (beside Cargo.toml)
├── .gitignore            # ignores /target, lib/, remappings.txt
└── src/
    ├── lib.rs            # a template contract
    └── bindings.rs       # generated trait bindings (empty until you `add`)
```

Refuses a non-empty directory unless `--force`.

### `rustereum add <owner/repo> [--ref <tag>]`

Import a GitHub Solidity dependency. This:

1. `git clone`s it (at `--ref`, if given) into `lib/<repo>/`,
2. records it in `rustereum.toml`,
3. appends the import remapping to `remappings.txt`,
4. generates Rust trait bindings (one `pub trait` per contract/interface, each
   carrying its `SOL_NAME` and `SOL_IMPORT`) into `src/bindings.rs`.

```console
$ rustereum add OpenZeppelin/openzeppelin-contracts --ref v5.1.0
```

### `rustereum fetch`

Reproduce a project's dependencies from `rustereum.toml`: clone anything missing
into `lib/` and regenerate `remappings.txt`. This is what CI runs before tests,
and what you run after cloning a project. Idempotent (skips already-cloned deps).

## `rustereum.toml`

```toml
[dependencies.openzeppelin-contracts]
git = "https://github.com/OpenZeppelin/openzeppelin-contracts"
rev = "v5.1.0"
remap = "@openzeppelin/contracts/=contracts/"
```

- The table key is the `lib/` clone directory name.
- `remap` is `<prefix>=<subpath>`; `fetch` turns it into the
  `remappings.txt` line `<prefix>=lib/<key>/<subpath>`, which the compiler passes
  to `solc` (with an absolute base path) so `import "@openzeppelin/..."` resolves.

The generated `bindings.rs` traits are what you implement to inherit a contract
(`impl Ownable for MyContract {}`); the macro reads their `SOL_IMPORT` const to
emit the right Solidity `import`, so it never touches the filesystem.

> `add` and `fetch` clone over the network; a `#[ignore]`d integration test
> covers the real clone path, so the default test run stays offline.
