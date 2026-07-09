# Rustereum Inheritance + CLI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Pivot rustereum to a Solidity backend and ship OZ inheritance (an Ownable counter gated by `onlyOwner`), plus a `rustereum` CLI that scaffolds projects and imports git dependencies with generated trait bindings.

**Architecture:** Every contract compiles IR → Solidity → `solc` (via foundry-compilers, with remappings) → bytecode + ABI, dumping `solc --ir` Yul for inspection. Inheritance is expressed as a Rust trait impl (`impl Ownable for Counter`); the generated `Ownable` binding carries the `.sol` import path in an associated const so the macro never touches the filesystem. The v1 hand-written Yul backend is retired.

**Tech Stack:** Rust (nightly), `syn`/`quote`/`proc-macro2`, `foundry-compilers` 0.11, `revm` 14, `alloy-primitives`, `clap` (CLI), `git` (shell out for `add`). Tests: `cargo nextest` + `cargo test` (trybuild/doctests).

**Reference design:** `docs/plans/2026-07-09-rustereum-inheritance-and-cli-design.md`

**Conventions (every task):** TDD — failing test first, watch it fail, minimal impl, watch it pass, commit. Run `cargo fmt` before committing. Commit messages `feat:`/`test:`/`chore:`/`refactor:`. `revm`-using tests run under `--features testing`. The first `solc` invocation with Solidity input may download a solc version — allow network + time.

**De-risking order:** Tasks 1–3 pivot the *existing* standalone counter/adder to the Solidity backend and prove it in revm BEFORE any inheritance machinery. Tasks 4–8 add inheritance. Task 9–10 the CLI.

---

### Task 1: `lower_solidity` for standalone contracts

Replace the Yul lowering with Solidity lowering for the *current* IR (no inheritance yet). This proves IR→Solidity in isolation.

**Files:**
- Create: `crates/rustereum/src/solidity.rs`
- Modify: `crates/rustereum/src/lib.rs` (add `pub mod solidity;`)

**Step 1: Write the failing test** (inline `#[cfg(test)] mod tests` in `solidity.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn counter() -> Contract {
        Contract {
            name: "Counter".into(),
            fields: vec![Field { name: "count".into(), ty: Type::U256 }],
            methods: vec![
                Method { name: "increment".into(), mutates: true, params: vec![], ret: None,
                    body: vec![Stmt::Assign { target: Place::Storage("count".into()), op: AssignOp::Add, value: Expr::Literal(1) }] },
                Method { name: "get".into(), mutates: false, params: vec![], ret: Some(Type::U256),
                    body: vec![Stmt::Return(Expr::StorageLoad("count".into()))] },
            ],
        }
    }

    #[test]
    fn emits_solidity_contract() {
        let src = lower_solidity(&counter());
        assert!(src.contains("pragma solidity"));
        assert!(src.contains("contract Counter {"));
        assert!(src.contains("uint256 count;"));
        assert!(src.contains("function increment() public {"));
        assert!(src.contains("count += 1;"));
        assert!(src.contains("function get() public view returns (uint256) {"));
        assert!(src.contains("return count;"));
    }
}
```

> NOTE: This task uses the CURRENT IR (no `params`/`modifiers`/`inherits`/`constructor` fields yet). Task 4 extends the IR and this function together. Write `lower_solidity` against today's `Contract`/`Method` shape.

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum solidity`
Expected: FAIL (`lower_solidity` undefined).

**Step 3: Implement `lower_solidity(&Contract) -> String`**

- Emit `// SPDX-License-Identifier: MIT` + `pragma solidity ^0.8.28;`.
- `contract <Name> {` … `}`.
- Fields → `<solidity_type> <name>;` (`Type::U256`→`uint256`). Add a `sol_type(&Type) -> &str` helper.
- Methods → `function <name>() public [view] [returns (T)] { <body> }`. Add `view` when `!mutates && ret.is_some()`. Storage vars are referenced bare (`count`, not `self.count`).
- Statement lowering: `Assign{Storage(f), Add, v}` → `<f> += <v>;`, `Assign{Storage(f), Set, v}` → `<f> = <v>;`, `Return(e)` → `return <e>;`, `ExprStmt(e)` → `<e>;`.
- Expr lowering: `Literal(n)` → `n`, `StorageLoad(f)` → `f`, `Binary{Add,l,r}` → `<l> + <r>`.
- Indent cleanly (4 spaces). This is the readable on-disk artifact.

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum solidity`
Expected: PASS.

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: add IR-to-Solidity lowering for standalone contracts"
```

---

### Task 2: Driver compiles Solidity (retire Yul backend)

Switch the driver from Yul standard-JSON to Solidity standard-JSON, request bytecode + ABI + IR, dump the solc-generated Yul.

**Files:**
- Modify: `crates/rustereum/src/driver.rs`
- Modify: `crates/rustereum/src/lib.rs` (remove `pub mod lower;`)
- Delete: `crates/rustereum/src/lower.rs`

**Step 1: Update the driver test** (in `driver.rs` `#[cfg(test)]`)

Keep the existing `counter()` fixture. Replace assertions so it verifies the Solidity path:

```rust
#[test]
fn compile_writes_solidity_yul_and_returns_bytecode() {
    let artifact = compile_contract(&counter()).expect("compile");
    // Solidity artifact written for inspection.
    assert!(artifact.sol_path.exists());
    assert!(std::fs::read_to_string(&artifact.sol_path).unwrap().contains("contract Counter"));
    // solc-generated Yul IR also dumped.
    assert!(artifact.yul_path.exists());
    // Real bytecode + real ABI (no longer empty).
    assert!(!artifact.bytecode.is_empty());
    assert!(artifact.abi.as_array().map(|a| !a.is_empty()).unwrap_or(false));
}
```

**Step 2: Run to verify it fails**

Run: `cargo nextest run -p rustereum driver`
Expected: FAIL (no `sol_path`; still Yul).

**Step 3: Rework the driver**

- `Artifact` gains `pub sol_path: PathBuf` (keep `yul_path`, `bytecode`, `abi`, `name`).
- `compile_contract`:
  1. `let sol = crate::solidity::lower_solidity(c);`
  2. Write `target/rustereum/<name>.sol` FIRST.
  3. Build a Solidity Standard-JSON input:
     ```json
     {
       "language": "Solidity",
       "sources": { "<name>.sol": { "content": "<sol>" } },
       "settings": {
         "outputSelection": { "*": { "*": ["evm.bytecode.object","abi","ir"] } },
         "optimizer": { "enabled": true }
       }
     }
     ```
     (For contracts with imports, remappings are added in Task 7 — for now, standalone has none.)
  4. `Solc::find_or_install(&Version::parse("0.8.28")?)` then `compile_as::<_, serde_json::Value>(&input)`.
  5. Surface fatal `errors` (severity == "error") with the `inspect target/rustereum/<name>.sol` hint.
  6. Extract from `contracts["<name>.sol"]["<Name>"]`: `evm.bytecode.object` (hex→bytes), `abi`, and `ir` (string). Index by name explicitly (do NOT use `.values().next()`).
  7. Write `<name>.yul` from the `ir` string (solc-generated Yul). If `ir` is empty, still write the file (may be empty on some solc configs — but request it; note if empty).
  8. Write `<name>.json` (bytecode hex + abi).
- Delete `crates/rustereum/src/lower.rs` and its `pub mod lower;` line. If `selector()` is used elsewhere (e.g. `testing.rs` calldata), MOVE a minimal `selector()` into `testing.rs` or a small `abi` helper module — grep first: `grep -rn "lower::selector\|crate::lower" crates/`.

> The `ir` output requires solc ≥ 0.8.13 and may need `"viaIR": true` in settings for full IR; if `ir` comes back empty, add `"viaIR": true` under settings and note the effect on bytecode. Prefer keeping `viaIR` off unless needed for the `.yul` dump.

**Step 4: Run to verify it passes**

Run: `cargo nextest run -p rustereum driver`
Expected: PASS (sol + yul written, real bytecode + non-empty abi).

**Step 5: Commit**

```bash
git add -A && git commit -m "refactor: compile via Solidity backend, retire hand-written Yul"
```

---

### Task 3: Update standalone examples + `TestEvm` selector to the Solidity backend

Make the existing counter/adder examples pass end-to-end through the new backend, and fix any `selector()` fallout.

**Files:**
- Modify: `crates/rustereum/src/testing.rs` (if it used `lower::selector`)
- Modify: `examples/src/counter.rs`, `examples/src/adder.rs` (likely unchanged, just verify)
- Modify: `crates/rustereum/tests/macro_expands.rs` if it referenced removed items

**Step 1:** Run the whole suite and see what broke:

Run: `cargo nextest run -p rustereum --features testing 2>&1; cargo nextest run -p examples 2>&1`
Expected: compile errors or failures pointing at removed `lower`/`selector`.

**Step 2: Fix selector.** `TestEvm::call`/`call_u256` need a 4-byte selector for `"get()"` etc. Add a local `fn selector(sig: &str) -> [u8;4]` in `testing.rs` that keccaks the signature (reuse `tiny_keccak`; canonicalize `"name()"` — for zero-arg sigs the string is already canonical). Keep it minimal.

**Step 3:** Ensure examples still assemble + compile + run in revm (counter 0→1→2, adder 0→10→20). The contracts and IR are unchanged; only the backend changed. The ABI-encoded calls must still work — with real solc ABI, `get()`/`increment()`/`add_ten()` selectors are computed the same way, so revm calls should succeed unchanged.

**Step 4: Run to verify**

Run: `cargo nextest run -p rustereum --features testing && cargo nextest run -p examples`
Expected: all PASS. Confirm `target/rustereum/Counter.sol` and `Counter.yul` exist.

**Step 5: Commit**

```bash
git add -A && git commit -m "test: standalone examples pass on the Solidity backend"
```

---

### Task 4: Extend the IR for inheritance, constructor, params, modifiers

**Files:**
- Modify: `crates/rustereum/src/ir.rs`
- Modify: `crates/rustereum/src/solidity.rs` (handle new fields; standalone output unchanged)

**Step 1: Write failing tests** (in `ir.rs`) asserting the new shapes construct:

```rust
#[test]
fn ir_supports_inheritance_and_constructor() {
    let c = Contract {
        name: "Counter".into(),
        inherits: vec![Parent {
            name: "Ownable".into(),
            import_path: "@openzeppelin/contracts/access/Ownable.sol".into(),
            base_args: vec!["initial_owner".into()],
        }],
        fields: vec![Field { name: "count".into(), ty: Type::U256 }],
        constructor: Some(Constructor {
            params: vec![Param { name: "initial_owner".into(), ty: Type::Address }],
            body: vec![],
        }),
        methods: vec![Method {
            name: "increment".into(), params: vec![], mutates: true, ret: None,
            modifiers: vec!["onlyOwner".into()],
            body: vec![Stmt::Assign { target: Place::Storage("count".into()), op: AssignOp::Add, value: Expr::Literal(1) }],
        }],
    };
    assert_eq!(c.inherits[0].name, "Ownable");
    assert_eq!(c.constructor.as_ref().unwrap().params[0].ty, Type::Address);
    assert_eq!(c.methods[0].modifiers, vec!["onlyOwner".to_string()]);
}
```

**Step 2: Run to verify it fails.** `cargo nextest run -p rustereum ir` → FAIL (fields/types missing).

**Step 3: Extend the IR** exactly per the design's "IR extensions":
- `enum Type { U256, Address, Bool }`
- `Contract` gains `inherits: Vec<Parent>` and `constructor: Option<Constructor>`.
- New `Parent { name, import_path, base_args }`, `Constructor { params, body }`, `Param { name, ty }`.
- `Method` gains `params: Vec<Param>` and `modifiers: Vec<String>`.
- Add a trait `ContractInherits { fn parents() -> Vec<Parent>; }` alongside the existing traits.
- Update `assemble::<T>()` in `lib.rs` to require `T: ContractStorage + ContractMethods + ContractInherits` and populate `inherits`, `constructor`, `methods`. (Constructor: see Task 5 for how the macro exposes it — for now `assemble` reads it from `ContractMethods` which will carry an optional constructor, OR add a `ContractMethods::constructor()`; pick one and keep consistent. RECOMMENDED: extend `ContractMethods` with `fn constructor() -> Option<Constructor> { None }` default.)
- Fix all existing constructors of `Contract`/`Method` across the codebase/tests to include the new fields (`inherits: vec![]`, `constructor: None`, `params: vec![]`, `modifiers: vec![]`). Grep: `grep -rn "Method {" crates/ examples/`.

**Step 4: Run to verify** `cargo nextest run -p rustereum` → all PASS (existing tests updated with new empty fields).

**Step 5: Commit** `git add -A && git commit -m "feat: extend IR for inheritance, constructor, params, modifiers"`

---

### Task 5: Extend `#[contract]` — params, `Address`, `#[constructor]`, `#[modifier]`, trait-impl arm

**Files:**
- Modify: `crates/rustereum-macros/src/lib.rs`
- Modify: `crates/rustereum/src/lib.rs` (add `Address` newtype + prelude export)
- Test: `crates/rustereum/tests/macro_inheritance.rs` (new)

**Step 1: Write the failing integration test** `tests/macro_inheritance.rs`:

```rust
use rustereum::prelude::*;
use rustereum::ir::{ContractInherits, ContractMethods, Type};

// Minimal local marker trait standing in for a generated binding.
pub trait Ownable {
    const SOL_NAME: &'static str = "Ownable";
    const SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol";
}

#[contract]
struct Counter { count: u256 }

#[contract]
impl Ownable for Counter {}

#[contract]
impl Counter {
    #[constructor(Ownable(initial_owner))]
    pub fn new(initial_owner: Address) {}

    #[modifier(onlyOwner)]
    pub fn increment(&mut self) { self.count += 1; }

    pub fn get(&self) -> u256 { self.count }
}

#[test]
fn macro_captures_inheritance_constructor_modifier() {
    let parents = <Counter as ContractInherits>::parents();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].name, "Ownable");
    assert_eq!(parents[0].import_path, "@openzeppelin/contracts/access/Ownable.sol");
    assert_eq!(parents[0].base_args, vec!["initial_owner".to_string()]);

    let ctor = <Counter as ContractMethods>::constructor().expect("ctor");
    assert_eq!(ctor.params[0].name, "initial_owner");
    assert_eq!(ctor.params[0].ty, Type::Address);

    let methods = <Counter as ContractMethods>::methods();
    let inc = methods.iter().find(|m| m.name == "increment").unwrap();
    assert_eq!(inc.modifiers, vec!["onlyOwner".to_string()]);
}
```

**Step 2: Run to verify it fails.** `cargo nextest run -p rustereum --test macro_inheritance` → FAIL.

**Step 3: Implement the macro extensions:**
- **Trait-impl arm:** in `contract`, match `Item::Impl` where `i.trait_.is_some()`. Emit the original impl `#i` plus `impl ::rustereum::ir::ContractInherits for #self_ty { fn parents() -> Vec<Parent> { vec![ Parent { name: <#self_ty as #trait_path>::SOL_NAME.to_string(), import_path: <#self_ty as #trait_path>::SOL_IMPORT.to_string(), base_args: vec![] } ] } }`. (base_args filled by merging with the constructor attr — see below. Simplest: `ContractInherits::parents()` returns name+path with empty base_args; `assemble` merges base_args from the constructor. OR have the constructor attr write base_args into the parent. Choose: **merge in `assemble`** by matching parent name.)
- **Inherent impl arm** (`i.trait_.is_none()`): as today, but now also:
  - Parse each fn's non-receiver params → `Param { name, ty }`. Type mapping: `u256`/`U256`→`Type::U256`, `Address`→`Type::Address`, `bool`→`Type::Bool`; else `compile_error!`.
  - `#[modifier(name)]` attribute on a fn → push `"name"` into that method's `modifiers`.
  - `#[constructor(Parent(arg1, arg2))]` attribute on a fn → treat as the constructor: its params → `Constructor.params`; body → `Constructor.body`; and record `(Parent → [arg names])` so `assemble` can attach base_args. Emit `fn constructor() -> Option<Constructor>` in the `ContractMethods` impl returning the built `Constructor`; also emit a `fn base_inits() -> Vec<(String, Vec<String>)>` (or fold into a richer return) so `assemble` merges base_args into the matching `Parent`. Keep the mechanism simple and documented.
  - A `#[constructor]` fn is NOT emitted as a normal method (exclude from `methods()`), but IS re-emitted in `#i` (native Rust still sees `pub fn new(...)`).
- **`ContractMethods` trait** gains `fn constructor() -> Option<Constructor> { None }` and (if used) `fn base_inits() -> Vec<(String, Vec<String>)> { vec![] }` with defaults, so non-inheriting contracts are unaffected.
- **`Address` newtype** in `lib.rs`: `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct Address(pub alloy_primitives::Address);` + prelude export. It only needs to exist so `initial_owner: Address` type-checks natively; no ops required.
- Keep absolute paths (`::rustereum::ir::...`), keep `compile_error!` discipline.

**Step 4: Run to verify it passes.** `cargo nextest run -p rustereum --test macro_inheritance` and the whole suite → PASS.

**Step 5: Commit** `git add -A && git commit -m "feat: macro supports params, Address, constructor, modifier, trait-impl inheritance"`

---

### Task 6: `lower_solidity` for inheritance, constructor, modifiers, camelCase

**Files:**
- Modify: `crates/rustereum/src/solidity.rs`

**Step 1: Write the failing test** (in `solidity.rs`) with a full Ownable-counter IR value (inherits + constructor + modifier + Address param):

```rust
#[test]
fn emits_inheriting_contract() {
    let c = /* Ownable counter IR: inherits Ownable(import path), constructor(initial_owner: Address) base Ownable(initial_owner), increment onlyOwner, get view */;
    let src = lower_solidity(&c);
    assert!(src.contains(r#"import "@openzeppelin/contracts/access/Ownable.sol";"#));
    assert!(src.contains("contract Counter is Ownable {"));
    assert!(src.contains("constructor(address initialOwner) Ownable(initialOwner) {}"));
    assert!(src.contains("function increment() public onlyOwner {"));
    assert!(src.contains("function get() public view returns (uint256) {"));
}
```

**Step 2: Run to verify it fails.** `cargo nextest run -p rustereum solidity` → FAIL.

**Step 3: Extend `lower_solidity`:**
- Emit one `import "<import_path>";` per `Parent`.
- Contract header: `contract <Name>[ is P1, P2] {`.
- Constructor: `constructor(<params>)[ <Parent>(<base_args>)] { <body> }`. Params → `<sol_type> <camelName>`. base_args → the camelCased arg names.
- Methods: append modifiers verbatim after visibility: `function <camelName>(<params>) public [view] [<mods...>] [returns (T)] { <body> }`.
- Add `Type::Address`→`address`, `Type::Bool`→`bool` to `sol_type`.
- **camelCase:** add `fn to_camel_case(&str) -> String` and apply to ALL rustereum-defined identifiers: field names, function names, param names, constructor param names, and base_arg names (they reference params, so must match). Do NOT camelCase: parent contract names (`Parent.name`) and modifier strings (`Method.modifiers`) — emit verbatim. Storage field references in bodies must use the SAME camelCased name as the field declaration (so `self.count` → `count`; if a field were `my_count` it'd be `myCount` in both the declaration and the body).

> IMPORTANT: body lowering references fields by name — ensure the camelCase transform is applied consistently to the field declaration AND every `StorageLoad`/`Place::Storage` reference, or storage names won't match. Centralize via a `camel(&str)` call at each identifier emission.

**Step 4: Run to verify it passes.** `cargo nextest run -p rustereum solidity` → PASS (both standalone and inheriting tests).

**Step 5: Commit** `git add -A && git commit -m "feat: Solidity codegen for inheritance, constructor, modifiers, camelCase"`

---

### Task 7: Driver project-context + remappings (compile inheriting contracts)

**Files:**
- Modify: `crates/rustereum/src/driver.rs`

**Step 1: Write the failing test.** Add a driver test that compiles an inheriting contract against a vendored OZ fixture. Create the fixture first: `crates/rustereum/tests/fixtures/oz/` containing minimal real `Ownable.sol` + `Context.sol` (copy from OpenZeppelin; ~40 lines total) and a `remappings.txt` line `@openzeppelin/contracts/=<abs-or-rel>/`. Then:

```rust
#[test]
fn compiles_inheriting_contract_with_remappings() {
    let c = /* Ownable counter IR (import path @openzeppelin/contracts/access/Ownable.sol) */;
    let opts = CompileOptions { project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project") };
    let artifact = compile_contract_with(&c, &opts).expect("compile");
    assert!(!artifact.bytecode.is_empty());
    // ABI includes owner() inherited from Ownable.
    let abi = artifact.abi.to_string();
    assert!(abi.contains("owner"));
}
```
Set up `tests/fixtures/project/` with `remappings.txt` + `lib/openzeppelin-contracts/contracts/{access/Ownable.sol,utils/Context.sol}` (vendored). The remapping maps `@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/`.

**Step 2: Run to verify it fails.** FAIL (`CompileOptions`/`compile_contract_with` undefined, or import resolution fails).

**Step 3: Implement project context + remappings:**
- `pub struct CompileOptions { pub project_root: PathBuf }` and `pub fn compile_contract_with(c: &Contract, opts: &CompileOptions) -> Result<Artifact, CompileError>`. Keep `compile_contract(c)` as `compile_contract_with(c, &CompileOptions { project_root: <cwd-upward search for remappings.txt, else cwd> })`.
- Read `<project_root>/remappings.txt` (lines `prefix=path`). Pass remappings into the Solidity Standard-JSON: add `"settings": { "remappings": ["@openzeppelin/contracts/=lib/openzeppelin-contracts/contracts/"], ... }`, and resolve import paths relative to `project_root`. foundry-compilers' `Solc::compile_as` with Standard-JSON honors a `remappings` array in settings; paths in `sources` and remapping targets are resolved by solc relative to its base path — set solc's `--base-path`/`--include-path` via foundry-compilers if needed, OR make remapping targets absolute (join `project_root`). Simplest robust approach: rewrite each remapping target to an ABSOLUTE path (`project_root.join(target)`) so solc finds files regardless of cwd. Verify solc resolves the OZ import and pulls in `Context.sol` transitively.
- On import-resolution failure, the solc error (`File not found`) is surfaced with an added hint: `did you run 'rustereum add'? (missing remapping/dependency)`.

**Step 4: Run to verify it passes.** PASS — inheriting contract compiles, ABI shows inherited `owner()`.

**Step 5: Commit** `git add -A && git commit -m "feat: driver resolves imports via project remappings"`

---

### Task 8: `TestEvm` — constructor args, caller selection, revert assertion

**Files:**
- Modify: `crates/rustereum/src/testing.rs`

**Step 1: Write the failing test** (in `testing.rs`, `--features testing`) using the vendored-OZ fixture project + the Ownable counter IR:

```rust
#[test]
fn ownable_counter_access_control() {
    let c = /* Ownable counter IR */;
    let opts = CompileOptions { project_root: /* fixtures/project */ };
    let artifact = compile_contract_with(&c, &opts).unwrap();

    let owner = Address::from([0x11; 20]);
    let stranger = Address::from([0x22; 20]);

    let mut evm = TestEvm::new();
    evm.fund(owner); evm.fund(stranger);
    let addr = evm.deploy_with(&artifact.bytecode, &[Token::Address(owner)]); // ctor arg
    evm.call_from(owner, addr, "increment()").expect("owner ok");
    assert_eq!(evm.call_u256(addr, "get()"), U256::from(1));
    assert!(evm.call_from(stranger, addr, "increment()").is_err()); // onlyOwner reverts
}
```

Define a minimal `Token` enum for constructor args (`Address(Address)`, `U256(U256)`) with ABI head-encoding (32-byte words) — only what the Ownable counter needs (one address). YAGNI.

**Step 2: Run to verify it fails.** FAIL.

**Step 3: Implement:**
- `deploy_with(&mut self, bytecode, args: &[Token]) -> Address`: ABI-encode `args` as 32-byte words, append to init code, run CREATE.
- `call_from(&mut self, caller: Address, to: Address, sig: &str) -> Result<(), ()>` (or a richer result): set tx caller, return `Err` on revert instead of panicking. Keep the existing `call`/`call_u256` (default caller) working.
- `fund(&mut self, addr)`: insert a funded account.
- `call_u256` unchanged.
- Reuse the local `selector()`.

**Step 4: Run to verify it passes.** PASS — owner increments, stranger reverts.

**Step 5: Commit** `git add -A && git commit -m "feat: TestEvm constructor args, caller selection, revert handling"`

---

### Task 9: `ownable_counter` example (showcase + e2e)

**Files:**
- Create: `examples/src/ownable_counter.rs`
- Create: `examples/fixtures/project/{remappings.txt, lib/openzeppelin-contracts/contracts/access/Ownable.sol, .../utils/Context.sol}` (vendored OZ)
- Create: `examples/src/bindings.rs` (the checked-in generated binding for `Ownable`, as if `rustereum add` produced it)
- Modify: `examples/src/lib.rs` (`pub mod bindings; pub mod ownable_counter;`)

**Step 1: Write the example** (`ownable_counter.rs`) exactly matching the design surface (`impl Ownable for Counter`, `#[constructor(Ownable(initial_owner))]`, `#[modifier(onlyOwner)]`), importing `Ownable` from `crate::bindings`. `bindings.rs`:
```rust
pub trait Ownable {
    const SOL_NAME: &'static str = "Ownable";
    const SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol";
}
```
Its `#[cfg(test)] mod tests` runs the full access-control e2e (owner ok, stranger reverts, `get()` progresses), pointing `CompileOptions` at `examples/fixtures/project`.

**Step 2: Run to verify.** `cargo nextest run -p examples ownable_counter` → after wiring, PASS. Confirm `target/rustereum/Counter.sol` (with `is Ownable`) and `Counter.yul` produced.

**Step 3: Commit** `git add -A && git commit -m "feat: ownable_counter example with real OZ inheritance e2e"`

---

### Task 10: `rustereum-cli` — `new` and `add`

**Files:**
- Create: `crates/rustereum-cli/Cargo.toml`, `crates/rustereum-cli/src/main.rs`
- Modify: root `Cargo.toml` (add member; add `clap` to workspace deps)

**Step 1 (new): failing test.** Add `crates/rustereum-cli/tests/cli.rs` using `assert_cmd` (add as dev-dep) or a direct function call. Prefer factoring logic into `lib`-style functions tested directly:
- `scaffold_new(dir) -> Result<()>` creates `Cargo.toml`, `src/lib.rs` (template contract), `lib/`, `remappings.txt`, `src/bindings.rs`.
- Test asserts those files exist and `remappings.txt`/`bindings.rs` are present.

```rust
#[test]
fn new_scaffolds_project() {
    let tmp = /* tempdir */;
    scaffold_new(tmp.path(), "myproj").unwrap();
    assert!(tmp.path().join("Cargo.toml").exists());
    assert!(tmp.path().join("remappings.txt").exists());
    assert!(tmp.path().join("src/bindings.rs").exists());
}
```

**Step 2: Run to verify it fails.** FAIL.

**Step 3: Implement `new`.** `clap` CLI with subcommands `new <name>` and `add <spec>`. `scaffold_new` writes the files (refuse non-empty dir without `--force`). Template contract = the standalone counter.

**Step 4 (add): failing test for the binding generator** (the network-free part). Factor `generate_bindings(sol_root) -> String` that scans `.sol` for `(abstract )?contract|interface <Name>` and emits trait bindings with `SOL_NAME` + `SOL_IMPORT`. Test against a small committed `.sol` fixture:
```rust
#[test]
fn generate_bindings_from_sol() {
    let out = generate_bindings(Path::new("tests/fixtures/oz-src"), "@openzeppelin/contracts/");
    assert!(out.contains("pub trait Ownable"));
    assert!(out.contains(r#"SOL_IMPORT: &'static str = "@openzeppelin/contracts/access/Ownable.sol""#));
}
```

**Step 5: Implement `add`.** `add <github-spec>`: shell out `git clone --depth 1 [--branch <ref>] https://github.com/<spec> lib/<repo>`; append remapping to `remappings.txt` (idempotent); call `generate_bindings` over the cloned `contracts/` and write/merge `src/bindings.rs`. The `git clone` path is exercised by a separate `#[ignore]`-by-default network test; the scan/codegen is covered by Step 4.

**Step 6: Run to verify.** `cargo nextest run -p rustereum-cli` → PASS (scaffold + bindgen; network test ignored). `cargo build --workspace` clean.

**Step 7: Commit** `git add -A && git commit -m "feat: rustereum CLI with new and add (git deps + binding generation)"`

---

## Final review

After all tasks: `cargo nextest run --workspace --features testing` + `cargo test --workspace` (doctests/trybuild) green; confirm `target/rustereum/Counter.sol` (with `is Ownable`) + `.yul` + `.json` produced; dispatch a final code reviewer over the whole diff; then superpowers:finishing-a-development-branch. Update the v1 design/README notes if the backend change affects documented behavior.
