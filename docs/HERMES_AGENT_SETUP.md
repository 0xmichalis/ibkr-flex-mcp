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

## Redeploying a new release

Hermes spawns the MCP server as a long-lived child process, so replacing the binary on disk changes
nothing until whatever spawned it restarts.

The catch is that "whatever spawned it" may be more than one thing. `hermes gateway` is the usual
case, but any other long-lived Hermes process configured with this server — `hermes dashboard`, for
instance — spawns its **own** copy, and `hermes gateway restart` does not touch it. Nothing else
will either: `mcp_stdio_watchdog.py` terminates its child only when the *spawning parent* dies, so
a parent that stays up keeps its original process — and its original binary — across any number of
deploys, while continuing to answer as though current.

So rather than assume a topology, ask the machine which units own a running copy:

```sh
for p in $(pgrep -u "$USER" -x ibkr-flex-mcp); do
  systemctl --user status "$p" 2>/dev/null | head -1 | grep -oE '[a-zA-Z0-9@._-]+\.service'
done | sort -u
```

On a gateway-only host that prints one unit; on a host also running the dashboard, two. Restart
what it lists. If it prints nothing, the server is not running under a systemd user unit — it may
be a foreground `hermes gateway run`, a launchd job on macOS, or spawned per-session by a client
such as Claude Code. Per-session clients need nothing: they pick up the new binary on their next
run.

```sh
version=v0.3.1
target=x86_64-unknown-linux-musl        # or aarch64-apple-darwin, x86_64-apple-darwin
asset="ibkr-flex-mcp-${version}-${target}.tar.gz"

gh release download "$version" --repo 0xmichalis/ibkr-flex-mcp --pattern "${asset}*"
sha256sum -c "${asset}.sha256"          # shasum -a 256 -c on macOS
tar xzf "$asset"

# Install via a temporary name and rename over the target. Writing in place fails
# with ETXTBSY while the old binary is running; rename is atomic and lets the
# running process keep its now-unlinked inode until it exits.
install -m 755 "ibkr-flex-mcp-${version}-${target}/ibkr-flex-mcp" ~/.local/bin/ibkr-flex-mcp.new
mv -f ~/.local/bin/ibkr-flex-mcp.new ~/.local/bin/ibkr-flex-mcp

# ...then restart each unit the command above listed, e.g.
systemctl --user restart hermes-gateway.service
```

Then confirm nothing is still on the old build. A process holding a replaced binary reports its
executable as deleted, which makes the check exact:

```sh
for p in $(pgrep -u "$USER" -x ibkr-flex-mcp); do
  t=$(readlink "/proc/$p/exe" 2>/dev/null) || continue
  case "$t" in *"(deleted)"*) echo "stale: $p -> $t";; esac
done
```

(Scoped to this binary on purpose. Sweeping every process instead turns up unrelated noise —
any browser or editor updated in place reports the same way — which buries the one line that
matters.)

Silence means every running copy is on the current binary. Comparing `sha256sum /proc/<pid>/exe`
against the installed file proves it positively — but when several copies are running, take the pid
from the owning unit rather than from `pgrep | head -1`. That returns the *lowest* pid, which is the
oldest process, and so is biased towards the stale one you are trying to rule out.
