# Examples

Each example is its own workspace crate with an end-to-end `revm` test. Run one
with:

```console
$ cargo nextest run -p <name> --features rustereum/testing
```

## Standalone

- [`counter`](counter/) — the canonical contract: a `u256` storage variable with
  `increment()` and `get()`.
- [`adder`](adder/) — showcases binary `+` in a body (`self.total = self.total + 10`).

## OpenZeppelin inheritance

See [`openzeppelin/`](openzeppelin/) for a gallery of contracts that inherit real
OpenZeppelin bases (`Ownable`, `Pausable`, `ReentrancyGuard`, `AccessControl`,
`ERC20`, `ERC721`, `ERC1155`, `ERC165`, `ERC1967Proxy`), plus which OpenZeppelin
areas aren't reachable yet and why. Those examples fetch their Solidity
dependencies first (`rustereum fetch`); the standalone examples above need no
dependencies.
