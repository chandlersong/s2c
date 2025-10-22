use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeRequestParam, InitializeResult, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{schemars, serde_json, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use rmcp_macros::{tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use yue::binance::bn_models::Ticker24hr;
use yue::binance::bn_restful_commands::execute_bn_get;
use yue::binance::history_data::CommonParam;
use yue::http_client::NonAuthRequestBuilder;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SymbolRequest {
    pub symbol: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolPrice {
    pub symbol: String,
    pub price: f64,
}

pub struct BinanceSpot {
    tool_router: ToolRouter<BinanceSpot>,
}

#[tool_router]
impl BinanceSpot {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "24 hour price change statistics for one symbol", name = "price_change_24h")]
    async fn price_change_24h(&self, Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>) -> Result<CallToolResult, McpError> {
        let ticker_24h_param = CommonParam::only_symbol(symbol.clone());
        let result = execute_bn_get::<CommonParam, NonAuthRequestBuilder, Ticker24hr>(
            &yue::binance::bn_restful_commands::SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND,
            Some(&ticker_24h_param),
            NonAuthRequestBuilder {},
        )
        .execute()
        .await;
        match result {
            Ok(ticker) => {
                let json_value = serde_json::to_value(&ticker).unwrap_or_else(|_| serde_json::json!({"error": "serialize failed"}));
                let content = Content::json(json_value);
                Ok(CallToolResult::success(vec![content?]))
            }
            Err(e) => {
                let err_msg = format!("Failed to fetch 24h price change: {}", e);
                Err(McpError::new(rmcp::model::ErrorCode::INTERNAL_ERROR, err_msg, None))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for BinanceSpot {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("this is a binance spot mcp. Tools: price_change_24h.".to_string()),
        }
    }

    async fn initialize(&self, _request: InitializeRequestParam, _: RequestContext<RoleServer>) -> Result<InitializeResult, McpError> {
        Ok(self.get_info())
    }
}
