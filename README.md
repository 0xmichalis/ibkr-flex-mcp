# ibkr-flex-mcp

A **read-only** [Model Context Protocol](https://modelcontextprotocol.io) server exposing your
Interactive Brokers account data via the [Flex Web Service](https://www.interactivebrokers.com/en/software/am/am/reports/flex_web_service_version_3.htm).

Read-only **by construction**: the Flex Web Service is a token-authenticated reporting API that
*cannot place, modify, or cancel orders*. There is no trading code in this server, so there is
no trading surface to misconfigure. Compromise of the token exposes statement reads only — not
your ability to trade.

## Why this exists

Most IBKR MCP servers wrap the TWS socket or Client Portal API: they need a live, logged-in
gateway and ship order-placement tools (often enabled by default). For an autonomous LLM agent
that is a real-money footgun. This server takes the opposite stance — the narrowest possible
read-only surface, a single static binary, and an audit-once codebase.

## Install

Prebuilt binaries are attached to each [GitHub Release](../../releases):

| Platform | Asset |
| --- | --- |
| Linux x86_64 | `ibkr-flex-mcp-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `ibkr-flex-mcp-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `ibkr-flex-mcp-vX.Y.Z-x86_64-apple-darwin.tar.gz` |

The Linux build is **static** (musl), so it links no libc and runs on any x86_64 Linux regardless
of the host's glibc version.

```sh
target=x86_64-unknown-linux-musl   # or aarch64-apple-darwin, x86_64-apple-darwin
tar xzf "ibkr-flex-mcp-vX.Y.Z-${target}.tar.gz"
install "ibkr-flex-mcp-vX.Y.Z-${target}/ibkr-flex-mcp" ~/.local/bin/
```

Each asset ships with a `.sha256` alongside it. On macOS the binary is unsigned, so Gatekeeper
will quarantine a downloaded copy — clear it with
`xattr -d com.apple.quarantine ~/.local/bin/ibkr-flex-mcp`.

Or build from source: `cargo build --release` (a static musl build uses
[`cross`](https://github.com/cross-rs/cross): `cross build --release --target x86_64-unknown-linux-musl`).

## Setup

1. In IBKR Client Portal → **Settings → Account Settings → Flex Web Service**: enable it and
   generate a **token**.
2. **Reports → Flex Queries → Activity Flex Query**: create one and note its **Query ID**. The
   query decides what the tools can see — see
   [Configuring the Flex query](docs/FLEX_QUERY_SETUP.md) for the sections, fields and period to
   set.
3. Provide `IBKR_FLEX_TOKEN` and `IBKR_FLEX_QUERY_ID` to the server, either as environment
   variables or in a `.env` file in the working directory (loaded via dotenvy; real environment
   variables take precedence). A `.env` is gitignored.

   ```sh
   # .env
   IBKR_FLEX_TOKEN=your_flex_web_service_token
   IBKR_FLEX_QUERY_ID=your_flex_query_id
   ```

## Tools

| Tool | Returns |
| --- | --- |
| `flex_run_query` | The configured Flex Query report as raw XML. |
| `flex_positions` | Open positions as structured JSON (symbol, quantity, mark price, cost basis, unrealized P&L). |
| `flex_trades` | Executions as structured JSON (date, buy/sell, open/close, quantity, price, commission, cost, realized P&L). |

All three are read-only, and each returns only what the Flex query is configured to emit — see
[Configuring the Flex query](docs/FLEX_QUERY_SETUP.md).

## Docs

- [Configuring the Flex query](docs/FLEX_QUERY_SETUP.md) — sections, fields, period, verification.
- [Use with a Hermes agent](docs/HERMES_AGENT_SETUP.md) — `~/.hermes/config.yaml` wiring.

## License

MIT — see [LICENSE](LICENSE).
