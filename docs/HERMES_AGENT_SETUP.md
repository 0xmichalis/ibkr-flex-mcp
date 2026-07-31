# Use with a Hermes agent

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

Verify: `hermes mcp test ibkr_flex` should connect and list the `flex_run_query`, `flex_positions`
and `flex_trades` tools.
