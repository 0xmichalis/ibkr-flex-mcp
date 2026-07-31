# Configuring the Flex query

A Flex query emits **only** the sections and fields you tick, so the query definition — not this
server — decides what the tools can see. The server always runs the one query named by
`IBKR_FLEX_QUERY_ID`; there are no per-call parameters, because the Flex Web Service accepts none
(`SendRequest` takes only token, query id and version — the date range is a property of the saved
query, and passing `fromDate`/`toDate`/`period` on the URL is silently ignored).

Everything below lives in IBKR Client Portal → **Performance & Reports → Flex Queries →
Activity Flex Query**.

## Sections

| Section | Tool | Configuration |
| --- | --- | --- |
| **Open Positions** | `flex_positions` | Options → *Summary* is enough (*Lot* also parses; it just yields one row per lot). |
| **Trades** | `flex_trades` | Options → tick **Executions**. *Orders* aggregates fills into one row and *Symbol Summary* collapses them further, so neither gives you true lot history. |

Anything else you enable is still reachable as raw XML through `flex_run_query`.

## Fields

Untick nothing you care about — a missing attribute is simply absent from the JSON, silently. The
fields the structured tools read:

- **Open Positions** — `symbol`, `description`, `assetCategory`, `currency`, `position`,
  `markPrice`, `positionValue`, `costBasisPrice`, `costBasisMoney`, `fifoPnlUnrealized`.
- **Trades** — `symbol`, `description`, `assetCategory`, `currency`, `tradeID`, `tradeDate`,
  `dateTime`, `settleDateTarget`, `levelOfDetail`, `buySell`, `openCloseIndicator`, `quantity`,
  `tradePrice`, `tradeMoney`, `proceeds`, `ibCommission`, `netCash`, `cost`, `fifoPnlRealized`,
  `exchange`, `notes`.

`levelOfDetail` is worth keeping: if you ever enable more than one Trades level, it is the only way
to tell execution rows from aggregates and avoid double counting. Execution rows carry
`levelOfDetail="EXECUTION"`.

## Period

Set the query's **Date Period** on the query's own page (not inside a section). The default,
*Last Business Day*, returns an empty `<Trades/>` on any day you did not trade — which looks
identical to "you have no trades". Use *Last 365 Calendar Days*, or a custom range reaching back to
when your oldest open position was opened, if you want the lot history behind current cost basis.

## Delivery configuration

Under the query's *Delivery Configuration*: **Models** = *Optional*, **Format** = *XML*,
**Period** as above. Leave `Include header and trailer records` and
`Include column headers` off — they add CSV-oriented noise the parser ignores.

## Verifying

With `IBKR_FLEX_TOKEN` and `IBKR_FLEX_QUERY_ID` in a repo-root `.env`:

```sh
cargo test --test live_flex -- --nocapture
```

This runs the real two-step fetch and prints one line per position and per trade, plus a summary
(`positions=N trades=M`). It is a no-op pass when the credentials are absent.

Two gotchas when a change to the query seems not to take effect:

- IBKR **caches a generated statement** for several minutes; a re-run can return the previous
  content with an identical `whenGenerated` timestamp. Wait, then re-run.
- The statement echoes what it actually used — check the `<FlexStatement ... period=... fromDate=...
  toDate=...>` attributes in `flex_run_query` output before concluding a section is empty.

## Reconciling positions against trades

Summing `flex_trades` quantities per symbol should reproduce the `flex_positions` quantity. When it
does not, the cause is usually the data, not a missing lot:

- **Symbol aliases.** The same instrument can appear under two symbols after a listing or venue
  change (e.g. `NHY` and `NHYo`), splitting one holding across two rows that net out together.
- **FX rows.** Currency conversions arrive as `Trade` rows (`CASH` asset category, symbols like
  `USD.CHF`) with no corresponding open position; the balance lives in the cash report.
- **Non-trade lots.** Fractional shares from dividend reinvestment, transfers in, and corporate
  actions never appear in Trades. A position with zero executions in the window is either older
  than the period or was not acquired by trading.
