# Rustereum — OZ Inheritance + CLI

**Date:** 2026-07-09
**Status:** Design approved, ready for implementation
**Builds on:** [v1 Rust→Yul DSL](2026-07-09-rustereum-rust-to-yul-dsl-design.md)

## Summary

Add a CLI that bootstraps rustereum projects and imports Solidity dependencies
(e.g. OpenZeppelin), and make rustereum contracts able to **inherit** those
Solidity contracts. Inheritance is expressed as Rust **trait implementation**;
the compiler pivots to a **Solidity backend** (Rust → Solidity → solc →
bytecode) so `solc` performs real inheritance. Inspectable Yul is preserved by
dumping `solc --ir` output.

First milestone: an **Ownable counter** — inherits OZ `Ownable`, `increment()`
gated by the inherited `onlyOwner` modifier, constructor forwarding
`initialOwner` to the parent.

## Goals

- Real, correct inheritance of unmodified OpenZeppelin Solidity.
- Idiomatic Rust surface: inheritance = `impl Parent for Contract`.
- A CLI to scaffold projects and fetch/import dependencies.
- Keep an inspectable on-disk artifact (now `.sol` + solc-generated `.yul`).

## Non-goals (this milestone — YAGNI)

- Calling inherited `internal` functions with arguments (that's ERC-20,
  milestone two). Marker traits only; no parent-API surface yet.
- `interface`-style external calls (the "interface" half of interop).
- Parsing modifiers/functions out of `.sol` (the scanner extracts only contract
  names + import paths).
- npm dependencies; plain-URL vendoring.
- Keeping the hand-written Yul backend (it is retired — see below).
- Mappings, events, `require`, `_msgSender()`, arithmetic beyond `+`.

## Key decision: unify on a Solidity backend

Real inheritance requires one compiler to see both languages. The tractable path
is to let `solc` do it: rustereum lowers its IR to **Solidity** that
`is Parent`, and `solc` compiles the whole hierarchy. Because rustereum's IR is
tiny, IR→Solidity is *simpler* than the v1 IR→Yul path and subsumes it.

**Every contract** (standalone or inheriting) now compiles
IR → Solidity → solc → bytecode. The v1 hand-written Yul backend (`lower.rs`) is
**retired**. Benefits: one codegen path; correct ABI encoding, dispatcher, and
revert handling for free; inspectable Yul preserved via `solc --ir`. Cost: no
direct control of the emitted Yul (acceptable — solc's is better than the v1
hand-rolled single-word-return Yul).

Rejected alternatives: a Solidity *frontend* lowering `.sol` into rustereum IR
(≈ reimplementing solc); Yul-object *merging* of independently compiled objects
(brittle slot/name/memory reconciliation).

## Surface syntax

Inheritance is a trait impl. The Ownable counter:

```rust
#[contract]
struct Counter {
    count: u256,
}

#[contract]
impl Ownable for Counter {}                    // inheritance = trait impl

#[contract]
impl Counter {
    #[constructor(Ownable(initial_owner))]     // base initializer
    pub fn new(initial_owner: Address) {}

    #[modifier(onlyOwner)]                      // inherited modifier, verbatim
    pub fn increment(&mut self) {
        self.count += 1;
    }

    pub fn get(&self) -> u256 {
        self.count
    }
}
```

- `impl Ownable for Counter {}` declares inheritance. Multiple parents =
  multiple trait impls. The `Ownable` trait is a **generated binding**
  (Section: CLI). For v1 it is an empty marker; later it can expose the parent's
  `internal` functions/modifiers as typed associated items.
- `#[constructor(Ownable(initial_owner))]` marks the constructor and records the
  base-initializer args, keyed by parent name.
- `#[modifier(onlyOwner)]` attaches an inherited modifier by name (emitted
  verbatim; solc resolves it from the parent).
- New DSL features this milestone requires: a **constructor**, **function
  parameters**, and an **`Address` type**.

## Architecture: macros produce IR, a function produces Solidity

Unchanged principle from v1: `#[contract]` lowers to a Rust-level IR via traits;
an ordinary function lowers IR → source. Only the backend target changes (Yul →
Solidity). `#[contract]` now handles **three** item shapes:

- **struct** → `ContractStorage` (name, fields).
- **inherent impl** (`impl Counter`) → `ContractMethods` (methods + constructor).
- **trait impl** (`impl Ownable for Counter`) → `ContractInherits` (parents).

All target the same type, linked by the trait system. `assemble::<T>()` gathers
all three into a `Contract`.

## IR extensions

```rust
enum Type { U256, Address, Bool }          // + Address, Bool

struct Contract {
    name: String,
    inherits: Vec<Parent>,                 // NEW
    fields: Vec<Field>,
    constructor: Option<Constructor>,      // NEW
    methods: Vec<Method>,
}

struct Parent {                            // NEW
    name: String,                          // "Ownable"
    import_path: String,                   // "@openzeppelin/contracts/access/Ownable.sol"
    base_args: Vec<String>,                // ["initial_owner"]
}

struct Constructor {                       // NEW
    params: Vec<Param>,
    body: Vec<Stmt>,                       // usually empty
}

struct Param { name: String, ty: Type }   // NEW — reused by methods

struct Method {
    name: String,
    params: Vec<Param>,                    // NEW
    mutates: bool,
    ret: Option<Type>,
    modifiers: Vec<String>,                // NEW — ["onlyOwner"], verbatim
    body: Vec<Stmt>,
}
```

- Slots are no longer assigned by rustereum — solc owns storage layout.
- `Parent.import_path` is **not** filled by the macro (no filesystem access). It
  is carried by the generated binding's associated const and resolved through
  the trait impl (see CLI section).
- `base_args` reference constructor param names; merged in from the
  `#[constructor(...)]` attribute, keyed by parent name.

## Solidity codegen (`lower_solidity`)

`lower_solidity(&Contract) -> String` emits standard Solidity. For the Ownable
counter:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import "@openzeppelin/contracts/access/Ownable.sol";

contract Counter is Ownable {
    uint256 count;

    constructor(address initialOwner) Ownable(initialOwner) {}

    function increment() public onlyOwner {
        count += 1;
    }

    function get() public view returns (uint256) {
        return count;
    }
}
```

Mapping rules:
- One `import "<path>";` per `Parent`; `contract <Name> is <Parent>, ...`.
- Fields → state variables (`U256`→`uint256`, `Address`→`address`,
  `Bool`→`bool`).
- `Constructor` → `constructor(<params>) <Parent>(<base_args>) { <body> }`.
- `Method` → `function <name>(<params>) public [view] [<modifiers>]
  [returns (T)] { <body> }`. `view` when `!mutates && ret.is_some()`.
- Statement/expression lowering (simpler than the Yul path): `self.count += 1`
  → `count += 1;`, `self.count` → `count`, `return e` → `return e;`, literals
  as-is.

**Identifier casing:** rustereum-defined identifiers (fields, params, functions,
constructor params) are converted **snake_case → camelCase**
(`initial_owner` → `initialOwner`, `get_count` → `getCount`, giving idiomatic ABI
selectors). Names that come *from* Solidity — parent contract names (`Ownable`)
and inherited modifier names (`onlyOwner`) — are emitted **verbatim** (they must
match the imported source). A `to_camel_case()` helper handles the former;
parent/modifier strings bypass it.

## Compilation driver

IR → `.sol` → solc-with-remappings → bytecode + ABI, dumping Yul:

1. `lower_solidity(&c)` → Solidity source.
2. Write `target/rustereum/<Name>.sol` **first** (before solc), so a solc
   failure leaves it inspectable.
3. Compile via `foundry-compilers` using a **Solidity project model**:
   `language: "Solidity"`, the generated source, plus `remappings.txt` + `lib/`
   so `import "@openzeppelin/..."` resolves. Request `evm.bytecode.object`,
   `abi`, and `ir`/`irOptimized` in `outputSelection`.
4. Write the solc-generated Yul IR to `target/rustereum/<Name>.yul` (inspectable
   Yul preserved).
5. Write `target/rustereum/<Name>.json` (bytecode + real ABI).

`compile_contract` gains **project context** — `CompileOptions { project_root }`
(the dir holding `remappings.txt` + `lib/`), defaulting to an upward search from
cwd. `lower.rs` and the hand-written `selector()` are removed (solc owns
dispatch).

## The CLI (`crates/rustereum-cli`)

Binary invoked as `rustereum`. Two commands:

**`rustereum new <name>`** — scaffold a cargo project depending on `rustereum`:
a template contract, an empty `lib/`, a `remappings.txt`, and an empty generated
`src/bindings.rs`. `lib/` is git-ignored (re-fetchable). Refuses a non-empty
target dir unless `--force`.

**`rustereum add <github-spec>`** (e.g. `OpenZeppelin/openzeppelin-contracts`):
1. **Fetch:** `git clone` (at a tag/commit) into `lib/<repo>/`.
2. **Remap:** append the remapping to `remappings.txt` (idempotent — skip
   duplicates). `foundry-compilers` reads it.
3. **Generate bindings:** lightly scan the dependency's `.sol` for
   `contract` / `abstract contract` / `interface <Name>`, and for each emit a
   Rust trait into `src/bindings.rs`:
   ```rust
   pub trait Ownable {
       const SOL_NAME: &'static str = "Ownable";
       const SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol";
   }
   ```

**Import path → compiler (no filesystem access in the macro):** because
`impl Ownable for Counter` makes `Counter: Ownable`, the macro-generated
`ContractInherits for Counter` emits
`Parent { name: <Counter as Ownable>::SOL_NAME.into(),
import_path: <Counter as Ownable>::SOL_IMPORT.into(), .. }`. The path is carried
by the binding's associated const, resolved through the trait impl — no runtime
registry, no rescanning. `base_args` merged from the `#[constructor(...)]`
attribute.

## Testing

- **`ownable_counter` example** — the e2e proof. Vendor the real OZ sources it
  needs (`Ownable.sol` + `Context.sol`) into a committed fixture `lib/` with a
  `remappings.txt` and a `bindings.rs` checked in *as if `rustereum add` ran* —
  proving genuine OZ inheritance, network-free in CI.
- **`TestEvm` extensions:** `deploy(bytecode, constructor_args)` (ABI-encode +
  append `initialOwner`), `call_from(caller, addr, sig)`, `expect_revert(...)`.
- **Access-control assertion:** deploy with `owner`; `increment()` as `owner`
  succeeds and `get()` reflects it; `increment()` as a stranger **reverts**
  (`onlyOwner` enforced in real bytecode). This exercises the whole new stack:
  trait-impl inheritance → binding import path → generated `is Ownable` Solidity
  → solc with remappings → real modifier enforcement → revm.
- **CLI tests:** `rustereum new` asserts scaffold files exist and the project
  `cargo build`s; `rustereum add`'s binding generator is unit-tested against a
  small committed `.sol` fixture (scan → expected trait output); the network
  `git clone` path is a separate, optionally-gated test.

## Error handling & diagnostics

- **Macro-time:** unknown param types, a `#[constructor(...)]` base-initializer
  naming an un-`impl`ed parent, malformed attributes → `compile_error!` at the
  span. Trait-impl inheritance gives a *free* check: a typo'd or un-`add`ed
  parent is a plain "cannot find trait `Ownable`".
- **Driver / solc-time:** `.sol` written before solc runs; failures hint
  `inspect target/rustereum/<Name>.sol`. Import-resolution failures surface
  solc's "File not found" plus a hint to run `rustereum add`. Both `.sol` and the
  `--ir` Yul land on disk.
- **CLI-time:** `add` reports git-clone failure explicitly; duplicate remapping
  is idempotent; a zero-contract scan warns rather than writing an empty file.
  `new` refuses a non-empty dir without `--force`.
- **Binding resolution:** an empty `SOL_IMPORT` errors before solc is invoked
  rather than emitting a broken import.

## Implementation order

1. Retire the Yul backend; add `lower_solidity` for the *existing* standalone
   counter/adder (no inheritance yet) — prove the Solidity pipeline end-to-end in
   revm, artifacts (`.sol`, solc `--ir` `.yul`, `.json`) produced. Update the v1
   examples to the new backend.
2. Extend the IR (Address/Bool, Parent, Constructor, Param, method
   params/modifiers).
3. Extend `#[contract]`: params, `Address` newtype, `#[constructor(...)]`,
   `#[modifier(...)]`, and the trait-impl arm → `ContractInherits`.
4. `lower_solidity` for inheritance/constructor/modifiers + camelCase.
5. Driver project-context + remappings + Solidity Standard-JSON + `--ir` dump.
6. `TestEvm` constructor-args / `call_from` / `expect_revert`.
7. `ownable_counter` example with vendored OZ fixtures + access-control e2e test.
8. `rustereum-cli`: `new`, then `add` (fetch + remap + bindgen), with tests.
