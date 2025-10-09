use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use rmcp_macros::{tool, tool_router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SymbolRequest {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolPrice {
    pub symbol: String,
    pub price: f64,
}

struct BinanceSpot {
    counter: Arc<Mutex<i32>>,
    tool_router: ToolRouter<BinanceSpot>,
}

#[tool_router]
impl BinanceSpot {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(Mutex::new(0)),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "get the latest price of a symbol")]
    fn current_price(&self, Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }
}
