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

Prebuilt **static** Linux binaries are attached to each [GitHub Release](../../releases) —
built for `x86_64-unknown-linux-musl`, so they link no libc and run on any x86_64 Linux
regardless of the host's glibc version.

```sh
tar xzf ibkr-flex-mcp-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz
install ibkr-flex-mcp-vX.Y.Z-x86_64-unknown-linux-musl/ibkr-flex-mcp ~/.local/bin/
```

Or build from source: `cargo build --release` (a static musl build uses
[`cross`](https://github.com/cross-rs/cross): `cross build --release --target x86_64-unknown-linux-musl`).

## Setup

1. In IBKR Client Portal → **Settings → Account Settings → Flex Web Service**: enable it and
   generate a **token**.
2. **Reports → Flex Queries**: create an *Activity Flex Query*, note its **Query ID**.
3. Provide `IBKR_FLEX_TOKEN` and `IBKR_FLEX_QUERY_ID` to the server, either as environment
   variables or in a `.env` file in the working directory (loaded via dotenvy; real environment
   variables take precedence). A `.env` is gitignored.

   ```sh
   # .env
   IBKR_FLEX_TOKEN=your_flex_web_service_token
   IBKR_FLEX_QUERY_ID=your_flex_query_id
   ```

## Use with a Hermes agent

Point your `~/.hermes/config.yaml` at the binary. Keep secrets in `~/.hermes/.env` and reference
them with `${VAR}` (Hermes interpolates from `~/.hermes/.env`):

```yaml
mcp_servers:
  ibkr_flex:
    command: /home/you/.local/bin/ibkr-flex-mcp
    args: []
    env:
      IBKR_FLEX_TOKEN: "${IBKR_FLEX_TOKEN}"
      IBKR_FLEX_QUERY_ID: "${IBKR_FLEX_QUERY_ID}"
    timeout: 120
    connect_timeout: 60
```

```sh
# ~/.hermes/.env  (chmod 600)
IBKR_FLEX_TOKEN=your_flex_web_service_token
IBKR_FLEX_QUERY_ID=your_flex_query_id
```

Verify: `hermes mcp test ibkr_flex` should connect and list the `flex_run_query` tool.

### Deploying to a remote server

Because the musl binary is static, deployment is just a copy — no toolchain, no Docker, no glibc
concerns on the target:

```sh
scp ibkr-flex-mcp you@server:~/.local/bin/
# then add the mcp_servers block above to the server's ~/.hermes/config.yaml + .env
```

## License

MIT — see [LICENSE](LICENSE).
