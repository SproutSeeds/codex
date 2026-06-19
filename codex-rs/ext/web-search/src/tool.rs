use codex_api::ReqwestTransport;
use codex_api::SearchClient;
use codex_api::SearchCommands;
use codex_api::SearchQuery;
use codex_api::SearchRequest;
use codex_api::SearchSettings;
use codex_core::web_search_action_detail;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::FunctionCallError;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema_without_compaction;
use codex_login::default_client::build_reqwest_client;
use codex_model_provider::SharedModelProvider;
use codex_protocol::items::WebSearchItem;
use codex_protocol::models::WebSearchAction;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolExposure;
use codex_tools::default_namespace_description;
use http::HeaderMap;
use serde_json::Value;
use url::Url;

use crate::history::recent_input;
use crate::output::SearchOutput;
use crate::schema::commands_schema;

pub(crate) const WEB_NAMESPACE: &str = "web";
pub(crate) const RUN_TOOL_NAME: &str = "run";
pub(crate) const WEB_SEARCH_TOOL_NAME: &str = "web_search";
const WEB_RUN_DESCRIPTION: &str = include_str!("../web_run_description.md");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebSearchToolPresentation {
    Namespace,
    Function,
}

pub(crate) struct WebSearchTool {
    pub(crate) session_id: String,
    pub(crate) provider: SharedModelProvider,
    pub(crate) settings: SearchSettings,
    pub(crate) presentation: WebSearchToolPresentation,
}

impl ToolExecutor<ToolCall> for WebSearchTool {
    fn tool_name(&self) -> ToolName {
        match self.presentation {
            WebSearchToolPresentation::Namespace => {
                ToolName::namespaced(WEB_NAMESPACE, RUN_TOOL_NAME)
            }
            WebSearchToolPresentation::Function => ToolName::plain(WEB_SEARCH_TOOL_NAME),
        }
    }

    fn spec(&self) -> ToolSpec {
        match self.presentation {
            WebSearchToolPresentation::Namespace => ToolSpec::Namespace(ResponsesApiNamespace {
                name: WEB_NAMESPACE.to_string(),
                description: default_namespace_description(WEB_NAMESPACE),
                tools: vec![ResponsesApiNamespaceTool::Function(responses_api_tool(
                    RUN_TOOL_NAME,
                ))],
            }),
            WebSearchToolPresentation::Function => {
                ToolSpec::Function(responses_api_tool(WEB_SEARCH_TOOL_NAME))
            }
        }
    }

    fn exposure(&self) -> ToolExposure {
        ToolExposure::Direct
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

fn responses_api_tool(name: &str) -> ResponsesApiTool {
    // Parse schema without compaction that removes field metadata/descriptions to match hosted tool definition.
    let parameters = match parse_tool_input_schema_without_compaction(&commands_schema()) {
        Ok(parameters) => parameters,
        Err(err) => panic!("search command schema should parse: {err}"),
    };

    ResponsesApiTool {
        name: name.to_string(),
        description: WEB_RUN_DESCRIPTION.to_string(),
        strict: false,
        parameters,
        output_schema: None,
        defer_loading: None,
    }
}

impl WebSearchTool {
    async fn handle_call(&self, call: ToolCall) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let commands = parse_commands(&call)?;
        let command_action = command_action(&commands);
        let provider = self
            .provider
            .api_provider()
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        let auth = self
            .provider
            .api_auth()
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        let client = SearchClient::new(
            ReqwestTransport::new(build_reqwest_client()),
            provider,
            auth,
        );
        let request = SearchRequest {
            id: self.session_id.clone(),
            model: call.model.clone(),
            reasoning: None,
            input: recent_input(call.conversation_history.items()),
            commands: Some(commands),
            settings: Some(self.settings.clone()),
            max_output_tokens: Some(
                u64::try_from(call.truncation_policy.token_budget()).unwrap_or(u64::MAX),
            ),
        };
        call.turn_item_emitter
            .emit_started(web_search_item(&call.call_id, WebSearchAction::Other))
            .await;
        let response = client
            .search(&request, HeaderMap::new())
            .await
            .map_err(|err| FunctionCallError::Fatal(err.to_string()))?;
        call.turn_item_emitter
            .emit_completed(web_search_item(&call.call_id, command_action))
            .await;

        Ok(Box::new(SearchOutput::new(response.output)))
    }
}

fn parse_commands(call: &ToolCall) -> Result<SearchCommands, FunctionCallError> {
    let arguments = call.function_arguments()?;
    parse_search_commands(arguments)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn parse_search_commands(arguments: &str) -> Result<SearchCommands, serde_json::Error> {
    if arguments.trim().is_empty() {
        return Ok(SearchCommands::default());
    }

    let original_err = match serde_json::from_str(arguments) {
        Ok(commands) => return Ok(commands),
        Err(err) => err,
    };
    let Some(commands) = parse_lenient_search_commands(arguments) else {
        return Err(original_err);
    };
    Ok(commands)
}

fn parse_lenient_search_commands(arguments: &str) -> Option<SearchCommands> {
    let value = serde_json::from_str::<Value>(arguments).ok()?;
    search_commands_from_lenient_value(value)
}

fn search_commands_from_lenient_value(value: Value) -> Option<SearchCommands> {
    match value {
        Value::String(query) => Some(SearchCommands {
            search_query: Some(vec![search_query(query)]),
            ..Default::default()
        }),
        Value::Array(values) => Some(SearchCommands {
            search_query: Some(query_values(values)?),
            ..Default::default()
        }),
        Value::Object(commands) if commands.contains_key("q") => {
            let query = search_query_from_value(Value::Object(commands))?;
            Some(SearchCommands {
                search_query: Some(vec![query]),
                ..Default::default()
            })
        }
        Value::Object(mut commands) => {
            normalize_query_field(&mut commands, "search_query")?;
            normalize_query_field(&mut commands, "image_query")?;
            serde_json::from_value(Value::Object(commands)).ok()
        }
        _ => None,
    }
}

fn normalize_query_field(commands: &mut serde_json::Map<String, Value>, field: &str) -> Option<()> {
    let Some(value) = commands.remove(field) else {
        return Some(());
    };
    let values = match value {
        Value::Array(values) => values,
        value => vec![value],
    };
    commands.insert(field.to_string(), Value::Array(query_json_values(values)?));
    Some(())
}

fn query_json_values(values: Vec<Value>) -> Option<Vec<Value>> {
    query_values(values)?
        .into_iter()
        .map(|query| serde_json::to_value(query).ok())
        .collect()
}

fn query_values(values: Vec<Value>) -> Option<Vec<SearchQuery>> {
    values.into_iter().map(search_query_from_value).collect()
}

fn search_query_from_value(value: Value) -> Option<SearchQuery> {
    match value {
        Value::String(query) => Some(search_query(query)),
        value => serde_json::from_value(value).ok(),
    }
}

fn search_query(query: String) -> SearchQuery {
    SearchQuery {
        q: query,
        recency: None,
        domains: None,
    }
}

fn command_action(commands: &SearchCommands) -> WebSearchAction {
    commands
        .search_query
        .as_deref()
        .and_then(query_action)
        .or_else(|| commands.image_query.as_deref().and_then(query_action))
        .or_else(|| {
            commands
                .open
                .as_deref()
                .and_then(|operations| operations.first())
                .and_then(|operation| {
                    literal_url(&operation.ref_id)
                        .map(|url| WebSearchAction::OpenPage { url: Some(url) })
                })
        })
        .or_else(|| {
            commands
                .find
                .as_deref()
                .and_then(|operations| operations.first())
                .map(|operation| WebSearchAction::FindInPage {
                    url: literal_url(&operation.ref_id),
                    pattern: Some(operation.pattern.clone()),
                })
        })
        .unwrap_or(WebSearchAction::Other)
}

fn query_action(queries: &[SearchQuery]) -> Option<WebSearchAction> {
    match queries {
        [] => None,
        [query] => Some(WebSearchAction::Search {
            query: Some(query.q.clone()),
            queries: None,
        }),
        queries => Some(WebSearchAction::Search {
            query: None,
            queries: Some(queries.iter().map(|query| query.q.clone()).collect()),
        }),
    }
}

fn literal_url(ref_id: &str) -> Option<String> {
    Url::parse(ref_id).is_ok().then(|| ref_id.to_string())
}

fn web_search_item(call_id: &str, action: WebSearchAction) -> ExtensionTurnItem {
    ExtensionTurnItem::WebSearch(WebSearchItem {
        id: call_id.to_string(),
        query: web_search_action_detail(&action),
        action,
    })
}

#[cfg(test)]
mod tests {
    use codex_api::SearchCommands;
    use codex_api::SearchQuery;
    use codex_protocol::models::WebSearchAction;
    use pretty_assertions::assert_eq;

    use super::command_action;
    use super::parse_search_commands;

    #[test]
    fn command_action_reports_queries_and_navigation_detail() {
        let cases = [
            (
                r#"{"image_query":[{"q":"waterfalls"},{"q":"mountains"}]}"#,
                WebSearchAction::Search {
                    query: None,
                    queries: Some(vec!["waterfalls".to_string(), "mountains".to_string()]),
                },
            ),
            (
                r#"{"open":[{"ref_id":"https://example.com/docs"}]}"#,
                WebSearchAction::OpenPage {
                    url: Some("https://example.com/docs".to_string()),
                },
            ),
            (
                r#"{"find":[{"ref_id":"https://example.com/docs","pattern":"install"}]}"#,
                WebSearchAction::FindInPage {
                    url: Some("https://example.com/docs".to_string()),
                    pattern: Some("install".to_string()),
                },
            ),
            (
                r#"{"find":[{"ref_id":"turn0search0","pattern":"install"}]}"#,
                WebSearchAction::FindInPage {
                    url: None,
                    pattern: Some("install".to_string()),
                },
            ),
            (
                r#"{"open":[{"ref_id":"turn0search0"}]}"#,
                WebSearchAction::Other,
            ),
        ];

        for (arguments, expected) in cases {
            let commands: SearchCommands =
                serde_json::from_str(arguments).expect("valid search command arguments");
            assert_eq!(command_action(&commands), expected);
        }
    }

    #[test]
    fn parse_search_commands_accepts_lenient_query_shapes() {
        let cases = [
            (
                r#""official OpenAI Codex documentation""#,
                SearchCommands {
                    search_query: Some(vec![search_query("official OpenAI Codex documentation")]),
                    ..Default::default()
                },
            ),
            (
                r#"{"search_query":"official OpenAI Codex documentation"}"#,
                SearchCommands {
                    search_query: Some(vec![search_query("official OpenAI Codex documentation")]),
                    ..Default::default()
                },
            ),
            (
                r#"{"search_query":["official OpenAI Codex documentation","OpenAI Codex CLI"]}"#,
                SearchCommands {
                    search_query: Some(vec![
                        search_query("official OpenAI Codex documentation"),
                        search_query("OpenAI Codex CLI"),
                    ]),
                    ..Default::default()
                },
            ),
            (
                r#"{"image_query":"OpenAI Codex screenshot"}"#,
                SearchCommands {
                    image_query: Some(vec![search_query("OpenAI Codex screenshot")]),
                    ..Default::default()
                },
            ),
        ];

        for (arguments, expected) in cases {
            assert_eq!(
                parse_search_commands(arguments).expect("lenient search command arguments"),
                expected
            );
        }
    }

    fn search_query(query: &str) -> SearchQuery {
        SearchQuery {
            q: query.to_string(),
            recency: None,
            domains: None,
        }
    }
}
