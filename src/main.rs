//! Entry point: read the Flex credentials from the environment and serve the MCP server
//! over stdio.
//!
//! Required environment variables (also loaded from a `.env` file in the working directory):
//!   IBKR_FLEX_TOKEN     — Flex Web Service token (Client Portal → Settings → Flex Web Service)
//!   IBKR_FLEX_QUERY_ID  — the Flex Query id to run (Reports → Flex Queries)

use ibkr_flex_mcp::flex::transport::ReqwestTransport;
use ibkr_flex_mcp::flex::FlexClient;
use ibkr_flex_mcp::mcp::FlexServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load a .env file if present; real environment variables take precedence.
    dotenvy::dotenv().ok();

    let token = std::env::var("IBKR_FLEX_TOKEN")
        .map_err(|_| "IBKR_FLEX_TOKEN must be set to your Flex Web Service token")?;
    let query_id = std::env::var("IBKR_FLEX_QUERY_ID")
        .map_err(|_| "IBKR_FLEX_QUERY_ID must be set to your Flex Query id")?;

    let client = FlexClient::new(ReqwestTransport::new()?);
    let server = FlexServer::new(client, token, query_id);

    // stdout carries the MCP protocol; keep it clean (no logging to stdout).
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
