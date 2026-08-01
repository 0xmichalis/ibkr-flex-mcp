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
| **Cash Report** | `flex_cash` | Options → tick **Currency Breakout** for one row per currency alongside the `BASE_SUMMARY` (base currency) total. Without it only the summary row arrives. |
| **Account Information** | `flex_cash` | Names the base currency the `BASE_SUMMARY` row is stated in. |

Anything else you enable is still reachable as raw XML through `flex_run_query`.

Note the *Cash Report* section (balances per currency) is not *Cash Transactions* (individual
deposits, dividends, fees); `flex_cash` reads the former. Neither section carries net
liquidation — for a NAV cross-check enable *Change in NAV* or *Net Asset Value (NAV) in Base*
and read it via `flex_run_query`.

## Fields

Untick nothing you care about — a missing attribute is simply absent from the JSON, silently. The
fields the structured tools read:

- **Open Positions** — `symbol`, `description`, `assetCategory`, `currency`, `position`,
  `markPrice`, `positionValue`, `costBasisPrice`, `costBasisMoney`, `fifoPnlUnrealized`.
- **Trades** — `symbol`, `description`, `assetCategory`, `currency`, `tradeID`, `tradeDate`,
  `dateTime`, `settleDateTarget`, `levelOfDetail`, `buySell`, `openCloseIndicator`, `quantity`,
  `tradePrice`, `tradeMoney`, `proceeds`, `ibCommission`, `netCash`, `cost`, `fifoPnlRealized`,
  `exchange`, `notes`.
- **Cash Report** — `currency`, `levelOfDetail`, `fromDate`, `toDate`, `startingCash`,
  `endingCash`, `endingSettledCash`.
- **Account Information** — `accountId`, `name`, `currency`.

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

This runs the real two-step fetch and prints one line per position, trade and cash currency, plus
a summary (`positions=N trades=M cash=K`). It is a no-op pass when the credentials are absent.

Two gotchas when a change to the query seems not to take effect:

- IBKR **caches a generated statement** for several minutes; a re-run can return the previous
  content with an identical `whenGenerated` timestamp. Wait, then re-run.
- The statement echoes what it actually used — check the `<FlexStatement ... period=... fromDate=...
  toDate=...>` attributes in `flex_run_query` output before concluding a section is empty.

## Keeping responses small

A year of executions is a large response — for one real account, 325 trades serialise to ~140 KB,
against ~540 KB for the raw statement XML. An MCP client with a bounded tool-output budget will
truncate that. `flex_trades` therefore takes filters:

| Parameter | Effect |
| --- | --- |
| `symbol` | Exact match, case-insensitive. `NHY` excludes the separately listed `NHYo`. |
| `since` / `until` | Inclusive `YYYYMMDD` bounds on `tradeDate`. Rows with no trade date are excluded when either is set. |
| `level_of_detail` | Usually `EXECUTION`, to drop aggregate rows. |
| `limit` | Cap the row count, keeping the most recent. |

Asking for one symbol's executions takes the same account from ~540 KB to under 4 KB.

Results are ordered by trade date ascending. IBKR does **not** emit `<Trade>` rows chronologically,
so this ordering — not statement order — is what makes `limit` mean "the most recent".

The reply carries `matched` alongside `returned`: if they differ, `limit` truncated the result.
Never read `returned` as a total.

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
