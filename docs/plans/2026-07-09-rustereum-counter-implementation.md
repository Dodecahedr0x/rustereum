# Rustereum Counter Milestone Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Ship the v1 pipeline that compiles a `#[contract]`-annotated Rust counter to Yul, then to EVM bytecode, and proves it works in `revm`.

**Architecture:** Proc macros lower Rust to a small IR (trait impls on the contract type); an ordinary function lowers IR → Yul; a driver writes the Yul to `target/rustereum/` and produces bytecode via `foundry-compilers`; correctness is asserted by executing bytecode in `revm`. Build the pipeline with a hand-written IR value first, then replace it with the macro.

**Tech Stack:** Rust (nightly toolchain present), `syn`/`quote`/`proc-macro2`, `foundry-compilers`, `revm`, `alloy-primitives`, `tiny-keccak`. Tests run with `cargo nextest`. `solc` is fetched/managed by `foundry-compilers` (network required on first compile).

**Reference design:** `docs/plans/2026-07-09-rustereum-rust-to-yul-dsl-design.md`

**Conventions for every task:**
- TDD: write the failing test first, watch it fail, implement minimally, watch it pass, commit.
- Run tests with `cargo nextest run -p <crate>` (fall back to `cargo test` for doctests / compile-fail).
- Keep `u256` as an alias to `alloy_primitives::U256`.
- Commit after each task with a `feat:`/`test:`/`chore:` message.

---

### Task 1: Workspace scaffolding

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/rustereum/Cargo.toml`, `crates/rustereum/src/lib.rs`
- Create: `crates/rustereum-macros/Cargo.toml`, `crates/rustereum-macros/src/lib.rs`
- Create: `examples/Cargo.toml`, `examples/src/lib.rs`

**Step 1: Root workspace manifest**

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = ["crates/rustereum", "crates/rustereum-macros", "examples"]

[workspace.dependencies]
syn = { version = "2", features = ["full"] }
quote = "1"
proc-macro2 = "1"
alloy-primitives = "0.8"
tiny-keccak = { version = "2", features = ["keccak"] }
foundry-compilers = "0.11"
revm = "14"
```

> Version numbers are starting points; if a version fails to resolve, the implementer picks the nearest working release and notes it. `foundry-compilers` and `revm` in particular move fast — pin whatever resolves and compiles.

**Step 2: `rustereum-macros` crate (proc-macro, empty for now)**

```toml
# crates/rustereum-macros/Cargo.toml
[package]
name = "rustereum-macros"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
syn = { workspace = true }
quote = { workspace = true }
proc-macro2 = { workspace = true }
```

```rust
// crates/rustereum-macros/src/lib.rs
// #[contract] attribute added in Task 6.
```

**Step 3: `rustereum` library crate**

```toml
# crates/rustereum/Cargo.toml
[package]
name = "rustereum"
version = "0.1.0"
edition = "2021"

[dependencies]
rustereum-macros = { path = "../rustereum-macros" }
alloy-primitives = { workspace = true }
tiny-keccak = { workspace = true }
foundry-compilers = { workspace = true }

[dev-dependencies]
revm = { workspace = true }
```

```rust
// crates/rustereum/src/lib.rs
pub mod ir;      // Task 2
pub mod lower;   // Task 3
pub mod driver;  // Task 4
pub mod testing; // Task 5

pub mod prelude {
    pub use crate::u256;
    pub use rustereum_macros::contract;
}

pub type u256 = alloy_primitives::U256;
```

> Create empty `ir.rs`, `lower.rs`, `driver.rs`, `testing.rs` with a `//` placeholder so the crate builds. Remove `pub use rustereum_macros::contract;` until Task 6 if it blocks compilation, or stub the macro in Task 1.

**Step 4: `examples` crate**

```toml
# examples/Cargo.toml
[package]
name = "examples"
version = "0.1.0"
edition = "2021"

[dependencies]
rustereum = { path = "../crates/rustereum" }

[dev-dependencies]
revm = { workspace = true }
```

```rust
// examples/src/lib.rs
// One module per showcased contract, added as features land.
```

**Step 5: Verify it builds, then commit**

Run: `cargo build --workspace`
Expected: builds clean (warnings about unused modules are fine).

```bash
git add -A
git commit -m "chore: scaffold rustereum workspace"
```

---

### Task 2: IR data model

**Files:**
- Modify: `crates/rustereum/src/ir.rs`
- Test: inline `#[cfg(test)]` module in `ir.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_ir_describes_two_methods() {
        let c = counter_contract();
        assert_eq!(c.name, "Counter");
        assert_eq!(c.fields.len(), 1);
        assert_eq!(c.fields[0].name, "count");
        assert_eq!(c.methods.len(), 2);
        let inc = &c.methods[0];
        assert_eq!(inc.name, "increment");
        assert!(inc.mutates);
        let get = &c.methods[1];
        assert_eq!(get.name, "get");
        assert!(!get.mutates);
        assert!(matches!(get.ret, Some(Type::U256)));
    }

    // Hand-written IR for the counter — the reference value the whole
    // pipeline is built against before the macro exists.
    fn counter_contract() -> Contract {
        Contract {
            name: "Counter".into(),
            fields: vec![Field { name: "count".into(), ty: Type::U256 }],
            methods: vec![
                Method {
                    name: "increment".into(),
                    mutates: true,
                    params: vec![],
                    ret: None,
                    body: vec![Stmt::Assign {
                        target: Place::Storage("count".into()),
                        op: AssignOp::Add,
                        value: Expr::Literal(1),
                    }],
                },
                Method {
                    name: "get".into(),
                    mutates: false,
                    params: vec![],
                    ret: Some(Type::U256),
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))],
                },
            ],
        }
    }
}
```

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum ir::tests`
Expected: FAIL (types not defined).

**Step 3: Implement the IR types**

Define exactly the types from the design's IR section in `ir.rs`: `Type`, `Field`, `Contract`, `Method`, `Stmt`, `Place`, `Expr`, `AssignOp`, `BinOp`. Derive `Debug, Clone, PartialEq` on all. Include `Expr::Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> }` and `BinOp::Add` even though the counter doesn't use `Binary` yet — it's in the model and Task 3 lowers it.

Also declare the two linking traits (used by the macro in Task 6, defined now so lowering can be generic):

```rust
pub trait ContractStorage { fn fields() -> Vec<Field>; fn name() -> String; }
pub trait ContractMethods { fn methods() -> Vec<Method>; }
```

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum ir::tests`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/rustereum/src/ir.rs
git commit -m "feat: add contract IR data model"
```

---

### Task 3: Lower IR → Yul

**Files:**
- Modify: `crates/rustereum/src/lower.rs`
- Test: inline `#[cfg(test)]` in `lower.rs`

**Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract { /* copy the hand-written counter from Task 2 */ }

    #[test]
    fn selector_matches_known_values() {
        // Canonical selectors from keccak256("increment()") / "get()".
        assert_eq!(selector("increment", &[]), 0xd09de08a);
        assert_eq!(selector("get", &[]), 0x6d4ce63c);
    }

    #[test]
    fn yul_contains_object_dispatch_and_bodies() {
        let yul = lower(&counter());
        assert!(yul.contains(r#"object "Counter""#));
        assert!(yul.contains(r#"object "runtime""#));
        assert!(yul.contains("datacopy(0, dataoffset(\"runtime\")"));
        assert!(yul.contains("switch selector"));
        assert!(yul.contains("case 0xd09de08a")); // increment
        assert!(yul.contains("case 0x6d4ce63c")); // get
        assert!(yul.contains("sstore(0, add(sload(0), 1))"));
        assert!(yul.contains("function fn_get() -> r"));
        assert!(yul.contains("sload(0)"));
    }
}
```

**Step 2: Run to verify they fail**

Run: `cargo nextest run -p rustereum lower::tests`
Expected: FAIL (`selector` / `lower` not defined).

**Step 3: Implement lowering**

- `pub fn selector(name: &str, params: &[Type]) -> u32`: build the canonical signature string (`"name(t1,t2)"`, where `Type::U256` → `"uint256"`), keccak256 it with `tiny_keccak`, take the first 4 bytes big-endian as `u32`.
- `pub fn lower(c: &Contract) -> String`: emit the Yul object from the design. Resolve each `Place::Storage(field)`/`StorageLoad(field)` to its slot = index of that field in `c.fields` (error/panic with a clear message if absent). Emit:
  - the constructor wrapper + `runtime` object,
  - selector read (`shr(224, calldataload(0))`),
  - a `switch` with one `case` per method: mutating/no-return → `fn_x() stop()`; returning u256 → `return_u256(fn_x())`,
  - each method body: lower `Stmt`/`Expr` recursively (`Assign Add` → `sstore(slot, add(sload(slot), <val>))`, `Assign Set` → `sstore(slot, <val>)`, `Return` → set the `r` out-var, `Literal(n)` → the number, `StorageLoad` → `sload(slot)`, `Binary Add` → `add(l, r)`),
  - the `return_u256` helper.
- Keep it string-building; a tiny indented-writer helper is fine. Don't over-engineer a Yul AST — YAGNI.

**Step 4: Run to verify they pass**

Run: `cargo nextest run -p rustereum lower::tests`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/rustereum/src/lower.rs
git commit -m "feat: lower contract IR to Yul"
```

---

### Task 4: Compilation driver (Yul file + bytecode)

**Files:**
- Modify: `crates/rustereum/src/driver.rs`
- Test: inline `#[cfg(test)]` in `driver.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract { /* hand-written counter */ }

    #[test]
    fn compile_writes_yul_and_returns_bytecode() {
        let artifact = compile_contract(&counter()).expect("compile");
        // Yul artifact is on disk for inspection.
        let yul_path = artifact.yul_path.clone();
        assert!(yul_path.exists(), "yul file must be written");
        assert!(std::fs::read_to_string(&yul_path).unwrap().contains("object \"Counter\""));
        // Bytecode is non-empty and looks like EVM init code.
        assert!(!artifact.bytecode.is_empty());
    }
}
```

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum driver::tests`
Expected: FAIL (`compile_contract` not defined). *(First `foundry-compilers` use downloads solc — allow network + time on this run.)*

**Step 3: Implement the driver**

- `pub struct Artifact { pub name: String, pub yul_path: PathBuf, pub bytecode: Vec<u8>, pub abi: serde_json::Value }` (or a typed ABI if convenient).
- `fn target_dir() -> PathBuf`: base from `OUT_DIR`, else `CARGO_TARGET_DIR`, else `./target`; join `rustereum`; `create_dir_all`.
- `pub fn compile_contract(c: &Contract) -> Result<Artifact, CompileError>`:
  1. `let yul = crate::lower::lower(c);`
  2. Write `target/rustereum/<name>.yul` **first**.
  3. Feed the Yul to `foundry-compilers` configured for Yul / strict-assembly input, pinning one solc version in a `const`. Extract deployed/creation bytecode + ABI.
  4. Write `target/rustereum/<name>.json` (bytecode hex + ABI).
  5. Return `Artifact`.
- `CompileError` enum: `Io`, `Solc`, with messages. On solc failure, include the hint `inspect target/rustereum/<name>.yul`.
- Add `serde_json` (and, if needed, `serde`) to `crates/rustereum/Cargo.toml`.

> The exact `foundry-compilers` API for feeding raw Yul may require the implementer to consult its docs (Solc input JSON with `language: "Yul"`). This is the one task with real external-API discovery — budget for it and keep the Yul-writing step independent so it's verifiable even if solc wiring takes iteration.

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum driver::tests`
Expected: PASS (bytecode non-empty, yul file present).

**Step 5: Commit**

```bash
git add -A
git commit -m "feat: driver writes Yul and compiles to bytecode"
```

---

### Task 5: `TestEvm` over revm

**Files:**
- Modify: `crates/rustereum/src/testing.rs`
- Test: inline `#[cfg(test)]` in `testing.rs` using the hand-written counter end-to-end

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;
    use crate::driver::compile_contract;

    fn counter() -> Contract { /* hand-written counter */ }

    #[test]
    fn counter_runs_in_evm() {
        let artifact = compile_contract(&counter()).unwrap();
        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(2));
    }
}
```

> `testing.rs` must be compiled for tests but uses `revm` (a dev-dependency). Gate the module with `#[cfg(any(test, feature = "testing"))]` in `lib.rs`, or move `revm` to a normal dependency behind a `testing` feature. Simplest for now: `#[cfg(test)]`-gate `TestEvm` usage and keep it available to the `examples` crate via a `testing` feature that pulls `revm`. The implementer picks whichever keeps `revm` out of the default user build; document the choice.

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum testing::tests`
Expected: FAIL (`TestEvm` not defined).

**Step 3: Implement `TestEvm`**

- `pub struct TestEvm { /* revm state */ }`.
- `new()`: EVM with an in-memory DB and a funded caller account.
- `deploy(&mut self, bytecode: &[u8]) -> Address`: run a create transaction with the init code; return the created address.
- `selector helper`: reuse `crate::lower::selector` by parsing `"name()"` → 4-byte big-endian prefix of calldata. For v1 only zero-arg signatures are needed; parse the name before `(`.
- `call(&mut self, addr, sig)`: send a transaction with the 4-byte selector as calldata; assert success.
- `call_u256(&mut self, addr, sig) -> U256`: like `call` but decode the 32-byte return value.
- Keep the revm surface minimal and localized here.

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum testing::tests`
Expected: PASS — the counter increments 0 → 1 → 2 in a real EVM.

**Step 5: Commit**

```bash
git add -A
git commit -m "feat: add TestEvm revm harness and end-to-end counter test"
```

---

### Task 6: `#[contract]` proc macro

**Files:**
- Modify: `crates/rustereum-macros/src/lib.rs`
- Test: `crates/rustereum-macros` unit tests where practical + integration via Task 7

**Step 1: Write the failing (integration) test scaffold**

In `crates/rustereum/tests/macro_expands.rs`:

```rust
use rustereum::prelude::*;
use rustereum::ir::{ContractStorage, ContractMethods, Type};

#[contract]
struct Counter { count: u256 }

#[contract]
impl Counter {
    pub fn increment(&mut self) { self.count += 1; }
    pub fn get(&self) -> u256 { self.count }
}

#[test]
fn macro_produces_expected_ir() {
    assert_eq!(<Counter as ContractStorage>::name(), "Counter");
    let fields = <Counter as ContractStorage>::fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "count");
    let methods = <Counter as ContractMethods>::methods();
    assert_eq!(methods.len(), 2);
    assert_eq!(methods[0].name, "increment");
    assert!(methods[0].mutates);
    assert!(matches!(methods[1].ret, Some(Type::U256)));
}
```

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum --test macro_expands`
Expected: FAIL (macro is a no-op / traits not implemented).

**Step 3: Implement `#[contract]`**

- Attribute macro `#[proc_macro_attribute] pub fn contract(attr, item)`.
- Parse `item` with `syn`. Branch:
  - **`syn::Item::Struct`** → generate `impl ContractStorage for <Name>` returning `fields()` (each field: name from ident, type must be `u256`/`U256` else `compile_error!` at the field's span) and `name()`. Re-emit the original struct unchanged.
  - **`syn::Item::Impl`** → generate `impl ContractMethods for <SelfTy>` where `methods()` builds the `Vec<Method>` by walking each `fn`: name from ident; `mutates` from `&mut self` receiver; params (v1: none besides receiver, else `compile_error!`); return type (`-> u256` → `Some(Type::U256)`, none → `None`, else error); body lowered statement-by-statement into IR:
    - `self.<field> += <intlit>;` → `Stmt::Assign { Storage(field), Add, Literal(n) }`
    - `self.<field> = <expr>;` → `Assign { .., Set, .. }`
    - `self.<field>` as a tail expr / `return self.<field>;` → `Stmt::Return(Expr::StorageLoad(field))`
    - integer literals → `Expr::Literal`; `a + b` → `Expr::Binary{Add,..}`; `self.<field>` in expr position → `Expr::StorageLoad`.
    - Anything else → `compile_error!` at that span with the "unsupported in v1" message.
  - Re-emit the original impl unchanged.
- Emit type paths as `::rustereum::ir::…` so resolution is import-independent.

> Building `Vec<Method>` inside generated code means `quote!`-ing IR constructor expressions. That's verbose but mechanical. Keep a private `to_tokens`-style helper in the macro crate that turns a parsed statement into the `::rustereum::ir::Stmt { .. }` token stream.

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum --test macro_expands`
Expected: PASS — macro-generated IR matches the hand-written IR's shape.

**Step 5: Cross-check macro IR against hand-written IR**

Add an assertion (same test file) that `<Counter as ContractMethods>::methods()` deep-equals the Task 2 hand-written counter methods (`assert_eq!` on the `Vec<Method>`). This is the proof the macro and the reference agree.

**Step 6: Commit**

```bash
git add -A
git commit -m "feat: add #[contract] proc macro lowering Rust to IR"
```

---

### Task 7: Counter example = showcase + test

**Files:**
- Create: `examples/src/counter.rs`
- Modify: `examples/src/lib.rs` (add `pub mod counter;`)
- Modify: `examples/Cargo.toml` if a `testing` feature/`revm` wiring is needed

**Step 1: Write the example with its own end-to-end test**

```rust
// examples/src/counter.rs
use rustereum::prelude::*;

#[contract]
pub struct Counter { count: u256 }

#[contract]
impl Counter {
    pub fn increment(&mut self) { self.count += 1; }
    pub fn get(&self) -> u256 { self.count }
}

#[cfg(test)]
mod tests {
    use super::Counter;
    use rustereum::ir::{ContractStorage, ContractMethods, Contract};
    use rustereum::driver::compile_contract;
    use rustereum::testing::TestEvm;
    use alloy_primitives::U256;

    fn contract() -> Contract {
        Contract {
            name: <Counter as ContractStorage>::name(),
            fields: <Counter as ContractStorage>::fields(),
            methods: <Counter as ContractMethods>::methods(),
        }
    }

    #[test]
    fn counter_end_to_end() {
        let artifact = compile_contract(&contract()).unwrap();
        assert!(artifact.yul_path.exists()); // inspectable Yul dropped in target/
        let mut evm = TestEvm::new();
        let addr = evm.deploy(&artifact.bytecode);
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(0));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
        evm.call(addr, "increment()");
        assert_eq!(evm.call_u256(addr, "get()"), U256::from(2));
    }
}
```

> If assembling `Contract` from the two traits is common, add a convenience `rustereum::assemble::<T>() -> Contract` (for `T: ContractStorage + ContractMethods`) and use it here. Small, DRY, worth it.

**Step 2: Run to verify it fails, then passes**

Run: `cargo nextest run -p examples`
Expected: FAIL first (module not wired), then PASS after implementation. Confirm `target/rustereum/Counter.yul` exists after the run and reads as expected.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat: add counter example as showcase and end-to-end test"
```

---

### Task 8: Macro diagnostics for unsupported syntax

**Files:**
- Create: `crates/rustereum/tests/ui/` compile-fail tests (via `trybuild`), or a documented manual check
- Modify: `crates/rustereum/Cargo.toml` (add `trybuild` dev-dep)

**Step 1: Write the failing compile-fail tests**

Add `trybuild` cases asserting each unsupported construct produces a clear `compile_error!`:
- a field typed `u128` (unsupported type),
- a method taking a parameter (unsupported in v1),
- a method body using `*` (unsupported operator) or a `let` binding.

```rust
// crates/rustereum/tests/ui.rs
#[test]
fn unsupported_syntax_is_rejected() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
```

Create one `tests/ui/<case>.rs` per rejection with a paired `.stderr` expectation.

**Step 2: Run to verify it fails**

Run: `cargo test -p rustereum --test ui`
Expected: FAIL until the macro emits precise spans/messages (Task 6 should already emit these; this task locks them with tests and fixes any gaps).

**Step 3: Tighten macro diagnostics until the UI tests pass**

Adjust `compile_error!` spans/messages in the macro so each case points at the offending token with the v1-subset message. Regenerate `.stderr` with `TRYBUILD=overwrite`.

**Step 4: Run to verify it passes**

Run: `cargo test -p rustereum --test ui`
Expected: PASS.

**Step 5: Commit**

```bash
git add -A
git commit -m "test: lock #[contract] diagnostics for unsupported syntax"
```

---

## Final review

After all tasks: dispatch a final code reviewer over the whole implementation, run `cargo nextest run --workspace` + `cargo test --workspace` (for doctests/UI) clean, confirm `target/rustereum/Counter.yul` and `.json` are produced, then use superpowers:finishing-a-development-branch.
