# OpenZeppelin examples

Each subdirectory is a self-contained rustereum project that **inherits a real
OpenZeppelin v5.1.0 contract** and is verified end-to-end in `revm`. Every
example is its own workspace crate with:

- `rustereum.toml` — declares the OpenZeppelin git dependency.
- `src/bindings.rs` — the generated trait binding(s) for the base(s) it inherits.
- `src/lib.rs` — the contract plus a `#[cfg(test)]` end-to-end test.

The OpenZeppelin sources themselves are **not committed** — run `rustereum fetch`
in a crate to clone them into a git-ignored `lib/` and generate its
`remappings.txt` before running that crate's tests:

```console
$ cargo build -p rustereum-cli
$ (cd examples/openzeppelin/erc20 && cargo run -p rustereum-cli -- fetch)
$ cargo nextest run -p erc20 --features rustereum/testing
```

## Gallery

| Example | Inherits | Import | What the test proves |
|---|---|---|---|
| [`ownable`](ownable/) | `Ownable` | `access/Ownable.sol` | `only_owner`-gated call: owner (≠ deployer) succeeds, stranger reverts |
| [`pausable`](pausable/) | `Pausable` | `utils/Pausable.sol` | `when_not_paused`-gated call passes while unpaused |
| [`reentrancy-guard`](reentrancy-guard/) | `ReentrancyGuard` | `utils/ReentrancyGuard.sol` | `non_reentrant`-gated call passes |
| [`access-control`](access-control/) | `AccessControl` | `access/AccessControl.sol` | deploys; `hasRole`/`grantRole` inherited into the ABI |
| [`erc20`](erc20/) | `ERC20("MyToken","MTK")` | `token/ERC20/ERC20.sol` | deploys; ABI has `transfer`/`balanceOf`/`totalSupply`/`approve` |
| [`erc721`](erc721/) | `ERC721("MyNFT","NFT")` | `token/ERC721/ERC721.sol` | deploys; ABI has `ownerOf`/`balanceOf`/`transferFrom` |
| [`erc1155`](erc1155/) | `ERC1155("…/{id}.json")` | `token/ERC1155/ERC1155.sol` | deploys; ABI has `balanceOf`/`safeTransferFrom` |
| [`erc165`](erc165/) | `ERC165` | `utils/introspection/ERC165.sol` | deploys; ABI has `supportsInterface` |
| [`erc1967-proxy`](erc1967-proxy/) | `ERC1967Proxy(impl, "")` | `proxy/ERC1967/ERC1967Proxy.sol` | deploys a `Logic` contract, then a proxy pointing at it |

Two patterns worth calling out:

- **Modifier gating** (`ownable`, `pausable`, `reentrancy-guard`) exercises an
  inherited modifier. `pausable`/`reentrancy-guard` only cover the happy path,
  because pausing / triggering re-entry needs internal calls the DSL can't
  express yet.
- **ABI proof** (`access-control`, the ERCs, `erc165`) shows that inheriting a
  base surfaces its functions in the compiled ABI, even though the typed client
  only exposes *your* contract's own methods.

## Not reachable yet

These OpenZeppelin areas need language features rustereum doesn't have:

- **Governance** (`Governor`) requires **overriding** abstract functions
  (`votingDelay`, `quorum`, `_getVotes`, `_countVote`, …) with real logic. The
  DSL can inherit a base and forward constructor args, but can't emit overriding
  function bodies, so the contract stays `abstract` and won't deploy
  (`solc: "Contract should be marked as abstract"`).
- **Interfaces** (`IERC20`) must be **implemented** with real bodies, or used to
  type **external calls** — neither is expressible today.

These map to two future features: *function overrides with real bodies* and
*external interface calls*.
