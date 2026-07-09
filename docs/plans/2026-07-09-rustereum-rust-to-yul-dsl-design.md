# Rustereum — a Rust DSL that compiles to Yul

**Date:** 2026-07-09
**Status:** Design approved, ready for implementation

## Summary

Rustereum is an embedded Rust DSL for writing EVM smart contracts. You write a
contract with real Rust structs and `impl` blocks annotated with `#[contract]`;
a procedural macro lowers them to an intermediate representation, and an ordinary
Rust function compiles that IR to Yul. The Yul is written to disk for inspection
and then compiled to EVM bytecode via `foundry-compilers`. Correctness is proven
by executing the bytecode in `revm`.

First milestone: a **counter with storage** (`increment()`, `get()`).

## Goals

- Write contracts in idiomatic, top-level Rust (no wrapping module).
- Emit readable Yul as an inspectable on-disk artifact.
- Produce deployable bytecode + ABI via existing, battle-tested tooling.
- Prove behavior end-to-end in an in-process EVM.

## Non-goals (v1 — YAGNI)

- Any type other than `u256`.
- Mappings, `Address`, events, `require`, `msg.sender`.
- Arithmetic beyond `+`, loops, `let` bindings, method calls.
- `STATICCALL` / view enforcement (recorded in ABI, not enforced).
- Multi-argument ABI encode/decode.
- Assembling bytecode ourselves (we target Yul precisely to avoid this).
- Yul-string snapshot tests (revm is the correctness signal).

## Surface syntax

Both the struct and the impl are top-level and carry `#[contract]`. No `mod`
wrapper. The macro branches on whether it is applied to a struct or an impl.

```rust
#[contract]
struct Counter {
    count: u256,
}

#[contract]
impl Counter {
    pub fn increment(&mut self) { self.count += 1; }
    pub fn get(&self) -> u256 { self.count }
}
```

## Architecture: macros produce IR, a normal function produces Yul

An attribute macro sees only the item it is attached to, so the `impl` macro
cannot see the struct's fields and vice versa. Rather than emit Yul from inside
the macros, the macros lower to a Rust-level IR and link through the trait
system:

- `#[contract]` on the **struct** generates `impl ContractStorage for Counter`
  describing the storage layout — field names, types, slot = declaration order.
- `#[contract]` on the **impl** generates `impl ContractMethods for Counter`
  describing each method as IR — name, mutability (`&self` vs `&mut self`),
  params, return type, and a statement/expression tree where `self.count`
  becomes `StorageRef("count")`.
- Both target the same type `Counter`, so the two halves link with **no shared
  global state and no macro-ordering hazard**.
- A plain function `compile::<Counter>() -> Result<Artifact, CompileError>`
  walks both traits, resolves field names → slots, and emits Yul.

The hard logic (IR → Yul) is therefore ordinary, unit-testable Rust rather than
macro code, and the "two items can't see each other" problem dissolves.

## IR data model

Minimal — just enough for the counter, with obvious room to grow. Slots are the
vector index (single source of truth), not stored per-field.

```rust
enum Type { U256 }                       // only u256 for now

struct Field { name: String, ty: Type }  // slot = index in the Vec

struct Contract {
    name: String,
    fields: Vec<Field>,
    methods: Vec<Method>,
}

struct Method {
    name: String,
    mutates: bool,                        // &mut self ⇒ true
    params: Vec<(String, Type)>,          // empty for counter
    ret: Option<Type>,
    body: Vec<Stmt>,
}

enum Stmt {
    Assign { target: Place, op: AssignOp, value: Expr },  // self.count += 1
    Return(Expr),                                          // return self.count
    ExprStmt(Expr),
}

enum Place { Storage(String) }           // self.<field>

enum Expr {
    StorageLoad(String),                 // self.count as a value
    Literal(u64),                        // 1
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
}

enum AssignOp { Set, Add }               // = and +=
enum BinOp { Add }
```

Each variant maps one-to-one to a Yul construct, so lowering is a straight
match. Anything outside this grammar is a macro-time compile error, never a
silent miscompilation.

## Yul output: object, dispatcher, ABI

`compile::<Counter>()` emits a Yul object in the shape `solc` expects — a
constructor wrapper whose `code` returns the runtime object.

```yul
object "Counter" {
    code {
        datacopy(0, dataoffset("runtime"), datasize("runtime"))
        return(0, datasize("runtime"))
    }
    object "runtime" {
        code {
            let selector := shr(224, calldataload(0))
            switch selector
            case 0xd09de08a /* increment() */ { fn_increment() stop() }
            case 0x6d4ce63c /* get() */        { return_u256(fn_get()) }
            default { revert(0, 0) }

            function fn_increment() { sstore(0, add(sload(0), 1)) }
            function fn_get() -> r  { r := sload(0) }
            function return_u256(v) { mstore(0, v) return(0, 32) }
        }
    }
}
```

- **Selectors:** `keccak256("increment()")[..4]`. The canonical signature is
  derived from the `Method` (name + param types), hashed with a keccak crate.
  The compiler owns this so callers and revm tests agree.
- **ABI:** zero-arg decode; single-word return via `mstore` + `return(0, 32)`.
  Multi-arg encode/decode deferred until a contract needs it.
- **View vs. mutating:** `mutates` is recorded in IR and surfaced as
  `stateMutability` in the ABI JSON, but not enforced with `STATICCALL` in v1.

## Compilation driver: IR → Yul file → bytecode

The macros only produce IR, so Yul generation and solc invocation happen when a
driver calls `compile::<T>()`. The driver is plain Rust and runs identically
from a test, a CLI, or `build.rs`.

1. `let yul = lower::<Counter>();` — walk the IR, emit the Yul object.
2. **Write `target/rustereum/Counter.yul` first**, before solc runs, so a solc
   failure still leaves inspectable Yul on disk. This satisfies the requirement
   that generated Yul always be available in the target folder.
3. **Compile to bytecode via `foundry-compilers`** with Yul/strict-assembly
   input settings; get deployed bytecode + ABI JSON. Structured errors, no
   shelling out.
4. Write `target/rustereum/Counter.json` (bytecode + ABI) alongside the `.yul`.

**Target dir resolution:** a single helper picks the base from `OUT_DIR` (under
`build.rs`) or `CARGO_TARGET_DIR` / `./target` (from tests/CLI), so artifacts
always land somewhere predictable.

**solc version:** `foundry-compilers` manages the solc binary (download/pin), so
there is no "is solc on PATH" fragility. One pinned version in one constant.

## Correctness via revm

The source of truth. Each example compiles through the full pipeline, then
executes the bytecode in an in-process EVM and asserts real behavior.

```rust
#[test]
fn counter_increments() {
    let artifact = compile::<Counter>().unwrap();   // bytecode + ABI

    let mut evm = TestEvm::new();
    let addr = evm.deploy(artifact.bytecode);

    assert_eq!(evm.call_u256(addr, "get()"), 0);
    evm.call(addr, "increment()");
    assert_eq!(evm.call_u256(addr, "get()"), 1);
    evm.call(addr, "increment()");
    assert_eq!(evm.call_u256(addr, "get()"), 2);
}
```

**`TestEvm`** is a thin wrapper over `revm`: `new()` builds an EVM with a funded
caller; `deploy(bytecode)` runs the constructor and returns the address;
`call` / `call_u256` ABI-encode the 4-byte selector (same keccak the compiler
uses), execute a transaction, and decode the returned word. This is the only
revm-specific surface, reused by every contract's tests.

This proves every layer at once — selector derivation, dispatcher switch, slot 0
`sload`/`sstore`, `add`, single-word ABI return. A failure surfaces as a
concrete value mismatch, not a vague error.

## Crate layout

Proc-macro code must live in its own crate. `examples/` is a real workspace
member that serves as both the showcase and the test suite.

```
rustereum/                     # workspace root
├── Cargo.toml                 # [workspace] members
├── crates/
│   ├── rustereum-macros/      # proc-macro = true — #[contract]
│   │   └── src/lib.rs         #   struct → impl ContractStorage
│   │                          #   impl   → impl ContractMethods (IR)
│   └── rustereum/             # main library (what users depend on)
│       └── src/
│           ├── lib.rs         #   re-exports #[contract], u256, prelude
│           ├── ir.rs          #   IR types + ContractStorage/Methods traits
│           ├── lower.rs       #   compile::<T>() — IR → Yul
│           ├── driver.rs      #   Yul → target/rustereum/*.yul + bytecode
│           └── testing.rs     #   TestEvm over revm
└── examples/                  # workspace member: showcase + test suite
    ├── Cargo.toml             #   depends on rustereum (+ revm dev-dep)
    └── src/
        ├── counter.rs         #   #[contract] Counter + #[test] via TestEvm
        └── …                  #   one file per showcased contract, self-testing
```

- `rustereum-macros` generates code referencing `rustereum` types via absolute
  paths (`::rustereum::ir::…`) so it resolves regardless of user imports.
  `rustereum` depends on `rustereum-macros` and re-exports `#[contract]`, giving
  users a single dependency and `use rustereum::prelude::*`.
- **Dependencies:** macros — `syn`, `quote`, `proc-macro2`. Library —
  `foundry-compilers`, a keccak crate, `revm` as a **dev-dependency** (testing
  only). `u256` is a type alias to `alloy_primitives::U256`, re-exported from the
  prelude.

### Examples are the tests

Each `examples/src/*.rs` file contains one contract written with `#[contract]`
*and* its own `#[test]` running the full pipeline and asserting that contract's
specific behavior. `cargo test -p examples` runs the whole corpus, and each test
drops its `target/rustereum/<Name>.yul` — the folder becomes a live gallery of
Rust-in / Yul-out pairs.

**Growth discipline:** an example may only use features the compiler supports,
so the examples set is an honest catalog of the language's capabilities. Adding a
language feature means adding an example that exercises it — showcase, tests, and
capability stay in lockstep. For v1 the corpus is just `counter.rs`.

## Error handling & diagnostics

Three layers, each failing loudly and early:

1. **Macro-time (unsupported syntax):** `#[contract]` emits `compile_error!` at
   the exact `syn` span for anything outside the supported grammar (non-`u256`
   type, unsupported operator, loop, `let`, method call). If it compiles, it is
   in the supported subset.
2. **Lowering-time:** `compile::<T>()` returns `Result<Artifact, CompileError>`.
   Internal-invariant failures (e.g. a `StorageRef` naming an absent field)
   carry a clear message with the contract/field name.
3. **Driver-time:** `foundry-compilers` and filesystem errors propagate as
   `CompileError` variants with context. Because the `.yul` is written before
   solc runs, a solc failure still leaves the artifact on disk with a message
   like `solc rejected generated Yul; inspect target/rustereum/Counter.yul`.

## Implementation order

1. Workspace scaffolding + crate skeletons.
2. IR types and the `ContractStorage` / `ContractMethods` traits (`ir.rs`).
3. `lower.rs`: IR → Yul for the counter's constructs (hand-write the IR first,
   before the macro, to test lowering in isolation).
4. `driver.rs`: write `.yul`, drive `foundry-compilers`, write `.json`.
5. `testing.rs`: `TestEvm` over revm.
6. `counter.rs` example with a hand-written IR value + revm test — proves the
   whole pipeline before any macro exists.
7. `rustereum-macros`: the `#[contract]` attribute (struct + impl arms),
   replacing the hand-written IR with generated IR.
8. Macro-time diagnostics for unsupported syntax.
