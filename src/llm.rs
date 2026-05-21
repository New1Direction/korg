//! Model-Agnostic LLM Provider Layer (`src/llm.rs`)
//!
//! A clean, vendor-agnostic LLM interface for driving Korg's Leader, Workers,
//! and Evaluator. Pure zero-SDK implementation using `reqwest` and `serde`.
//! Features built-in observability, exponential backoff, and a circuit breaker.

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::{self, BoxStream};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// =========================================================================
// Core Data Structures
// =========================================================================

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String, // typically "function"
    pub function: FunctionCall,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String, // JSON-encoded string
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON schema object
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum MultiModalContent {
    Image {
        bytes: Vec<u8>,
        mime_type: String,
        description: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct LlmRequest {
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub stop_sequences: Option<Vec<String>>,
    pub multimodal: Option<Vec<MultiModalContent>>,
    
    // Provenance / policy metadata
    pub tx_id: Option<String>,
    pub session_id: Option<String>,
    pub policy_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Other(String),
}

#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub content: String,
    pub usage: TokenUsage,
    pub model: String,
    pub finish_reason: FinishReason,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmDelta {
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<FinishReason>,
}

// =========================================================================
// Error Definitions
// =========================================================================

#[derive(Clone, Debug)]
pub enum LlmError {
    Http { status: u16, body: String },
    Timeout(String),
    RateLimit(String),
    Auth(String),
    Parser(String),
    Network(String),
    CircuitBreakerOpen,
    Unknown(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http { status, body } => write!(f, "HTTP error (status {}): {}", status, body),
            Self::Timeout(msg) => write!(f, "Timeout error: {}", msg),
            Self::RateLimit(msg) => write!(f, "Rate limit error: {}", msg),
            Self::Auth(msg) => write!(f, "Authentication error: {}", msg),
            Self::Parser(msg) => write!(f, "Response parsing error: {}", msg),
            Self::Network(msg) => write!(f, "Network error: {}", msg),
            Self::CircuitBreakerOpen => write!(f, "Circuit breaker is open"),
            Self::Unknown(msg) => write!(f, "Unknown LLM error: {}", msg),
        }
    }
}

impl std::error::Error for LlmError {}

// =========================================================================
// Trait Definition
// =========================================================================

#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError>;
}

// =========================================================================
// 1. Mock Provider (Offline testing)
// =========================================================================

pub struct MockProvider {
    pub name: &'static str,
    pub next_responses: Arc<Mutex<std::collections::VecDeque<Result<LlmResponse, LlmError>>>>,
    pub next_stream: Arc<Mutex<Option<Vec<LlmDelta>>>>,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            name: "mock-offline",
            next_responses: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            next_stream: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_response(&self, response: Result<LlmResponse, LlmError>) {
        self.next_responses.lock().unwrap().push_back(response);
    }

    pub fn set_stream_deltas(&self, deltas: Vec<LlmDelta>) {
        *self.next_stream.lock().unwrap() = Some(deltas);
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        if let Some(resp) = self.next_responses.lock().unwrap().pop_front() {
            resp
        } else {
            // Default mock response based on user inputs
            let input_summary = req.messages.last().map(|m| m.content.as_str()).unwrap_or("");
            let content = format!("[Mock Response to: \"{}\"]", input_summary);
            Ok(LlmResponse {
                content,
                usage: TokenUsage {
                    prompt_tokens: 15,
                    completion_tokens: 10,
                    total_tokens: 25,
                },
                model: "mock-model-v1".to_string(),
                finish_reason: FinishReason::Stop,
                tool_calls: None,
            })
        }
    }

    async fn complete_stream(
        &self,
        _req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        let deltas = if let Some(stream_data) = self.next_stream.lock().unwrap().take() {
            stream_data
        } else {
            vec![
                LlmDelta { content: "Mock ".to_string(), tool_calls: None, finish_reason: None },
                LlmDelta { content: "stream ".to_string(), tool_calls: None, finish_reason: None },
                LlmDelta { content: "reply.".to_string(), tool_calls: None, finish_reason: Some(FinishReason::Stop) },
            ]
        };

        let stream_items: Vec<Result<LlmDelta, LlmError>> = deltas.into_iter().map(Ok).collect();
        Ok(Box::pin(stream::iter(stream_items)))
    }
}

// =========================================================================
// SSE Helper Stream Line Parser
// =========================================================================

fn create_sse_stream<F>(
    mut bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + Unpin + 'static,
    parse_line: F,
) -> BoxStream<'static, Result<LlmDelta, LlmError>>
where
    F: Fn(&str) -> Option<Result<LlmDelta, LlmError>> + Send + Sync + 'static,
{
    let mut buffer = Vec::new();
    let parse_line = Arc::new(parse_line);

    let stream = stream::unfold(
        (bytes_stream, buffer, parse_line),
        move |(mut stream, mut buffer, parse_line)| async move {
            loop {
                // Check if buffer contains any complete lines
                if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes = buffer.drain(..=pos).collect::<Vec<u8>>();
                    if let Ok(line_str) = std::str::from_utf8(&line_bytes) {
                        let trimmed = line_str.trim();
                        if !trimmed.is_empty() {
                            if let Some(res) = parse_line(trimmed) {
                                return Some((res, (stream, buffer, parse_line)));
                            }
                        }
                    }
                } else {
                    // Fetch next chunk of bytes
                    match stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.extend_from_slice(&bytes);
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(LlmError::Network(format!("Stream error: {}", e))),
                                (stream, buffer, parse_line),
                            ));
                        }
                        None => {
                            // Process remaining buffer if it doesn't end with a newline
                            if !buffer.is_empty() {
                                if let Ok(line_str) = std::str::from_utf8(&buffer) {
                                    let trimmed = line_str.trim();
                                    if !trimmed.is_empty() {
                                        if let Some(res) = parse_line(trimmed) {
                                            buffer.clear();
                                            return Some((res, (stream, buffer, parse_line)));
                                        }
                                    }
                                }
                                buffer.clear();
                            }
                            return None; // Stream fully exhausted
                        }
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

// =========================================================================
// 2. OpenAI Provider (Pure HTTP Adapter)
// =========================================================================

pub struct OpenAIProvider {
    pub client: reqwest::Client,
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String, base_url: Option<String>, default_model: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            default_model: default_model.unwrap_or_else(|| "gpt-4o".to_string()),
        }
    }

    fn serialize_request(&self, req: LlmRequest, stream: bool) -> serde_json::Value {
        let mut tools_val = None;
        if let Some(tools) = req.tools {
            let mapped: Vec<serde_json::Value> = tools
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            tools_val = Some(serde_json::Value::Array(mapped));
        }

        let mut body = serde_json::json!({
            "model": self.default_model,
            "messages": req.messages,
            "temperature": req.temperature,
            "stream": stream
        });

        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(t) = tools_val {
            body["tools"] = t;
        }
        if let Some(stop) = req.stop_sequences {
            body["stop"] = serde_json::json!(stop);
        }

        body
    }

    fn parse_response(&self, val: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let choice = val
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| LlmError::Parser("No choices returned in OpenAI response".to_string()))?;

        let message = choice
            .get("message")
            .ok_or_else(|| LlmError::Parser("No message returned in choice".to_string()))?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let tool_calls = message.get("tool_calls").and_then(|tc| {
            serde_json::from_value::<Vec<ToolCall>>(tc.clone()).ok()
        });

        let finish_reason_str = choice.get("finish_reason").and_then(|f| f.as_str()).unwrap_or("stop");
        let finish_reason = match finish_reason_str {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            other => FinishReason::Other(other.to_string()),
        };

        let usage = val.get("usage").and_then(|u| {
            serde_json::from_value::<TokenUsage>(u.clone()).ok()
        }).unwrap_or(TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        let model = val
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.default_model)
            .to_string();

        Ok(LlmResponse {
            content,
            usage,
            model,
            finish_reason,
            tool_calls,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = self.serialize_request(req, false);

        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let body_json = res
            .json::<serde_json::Value>()
            .await
            .map_err(|e| LlmError::Parser(format!("Failed to parse response JSON: {}", e)))?;

        self.parse_response(body_json)
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = self.serialize_request(req, true);

        let res = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let bytes_stream = res.bytes_stream();
        let parse_line = |line: &str| -> Option<Result<LlmDelta, LlmError>> {
            if !line.starts_with("data: ") {
                return None;
            }
            let data = &line["data: ".len()..];
            if data == "[DONE]" {
                return None;
            }

            match serde_json::from_str::<serde_json::Value>(data) {
                Ok(val) => {
                    let choice = val.get("choices")?.as_array()?.first()?;
                    let delta = choice.get("delta")?;
                    let content = delta.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                    let tool_calls = delta.get("tool_calls").and_then(|tc| {
                        serde_json::from_value::<Vec<ToolCall>>(tc.clone()).ok()
                    });
                    let finish_reason_str = choice.get("finish_reason").and_then(|f| f.as_str());
                    let finish_reason = finish_reason_str.map(|fr| match fr {
                        "stop" => FinishReason::Stop,
                        "length" => FinishReason::Length,
                        "tool_calls" => FinishReason::ToolCalls,
                        "content_filter" => FinishReason::ContentFilter,
                        other => FinishReason::Other(other.to_string()),
                    });

                    Some(Ok(LlmDelta {
                        content,
                        tool_calls,
                        finish_reason,
                    }))
                }
                Err(e) => Some(Err(LlmError::Parser(format!("Failed to parse SSE line: {}", e)))),
            }
        };

        Ok(create_sse_stream(bytes_stream, parse_line))
    }
}

// =========================================================================
// 3. Anthropic Provider (Claude HTTP Messages API)
// =========================================================================

pub struct AnthropicProvider {
    pub client: reqwest::Client,
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>, default_model: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com/v1".to_string()),
            default_model: default_model.unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string()),
        }
    }

    fn serialize_request(&self, req: LlmRequest, stream: bool) -> serde_json::Value {
        // Extract system messages, since Anthropic requires a top-level system parameter
        let mut system_parts = Vec::new();
        let mut filtered_messages = Vec::new();

        for m in req.messages {
            if m.role == Role::System {
                system_parts.push(m.content);
            } else {
                // Anthropic maps 'assistant' role to 'assistant', and 'user' to 'user'
                let anthropic_role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user", // Tool is mapped to user with special block types in Anthropic generally
                };
                filtered_messages.push(serde_json::json!({
                    "role": anthropic_role,
                    "content": m.content
                }));
            }
        }

        let mut body = serde_json::json!({
            "model": self.default_model,
            "messages": filtered_messages,
            "temperature": req.temperature,
            "stream": stream
        });

        // Set system prompt if present
        if !system_parts.is_empty() {
            body["system"] = serde_json::json!(system_parts.join("\n\n"));
        }

        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        } else {
            body["max_tokens"] = serde_json::json!(4096); // Anthropic requires max_tokens
        }

        if let Some(tools) = req.tools {
            let mapped: Vec<serde_json::Value> = tools
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(mapped);
        }

        if let Some(stop) = req.stop_sequences {
            body["stop_sequences"] = serde_json::json!(stop);
        }

        body
    }

    fn parse_response(&self, val: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let content_blocks = val
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| LlmError::Parser("No content blocks returned from Anthropic".to_string()))?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for block in content_blocks {
            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("text");
            if block_type == "text" {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    content.push_str(text);
                }
            } else if block_type == "tool_use" {
                if let (Some(id), Some(name), Some(input)) = (
                    block.get("id").and_then(|i| i.as_str()),
                    block.get("name").and_then(|n| n.as_str()),
                    block.get("input"),
                ) {
                    tool_calls.push(ToolCall {
                        id: id.to_string(),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: name.to_string(),
                            arguments: input.to_string(),
                        },
                    });
                }
            }
        }

        let stop_reason_str = val.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("end_turn");
        let finish_reason = match stop_reason_str {
            "end_turn" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            other => FinishReason::Other(other.to_string()),
        };

        // Usage mapping
        let usage_val = val.get("usage");
        let input_tokens = usage_val.and_then(|u| u.get("input_tokens")).and_then(|t| t.as_u64()).unwrap_or(0) as u32;
        let output_tokens = usage_val.and_then(|u| u.get("output_tokens")).and_then(|t| t.as_u64()).unwrap_or(0) as u32;
        let usage = TokenUsage {
            prompt_tokens: input_tokens,
            completion_tokens: output_tokens,
            total_tokens: input_tokens + output_tokens,
        };

        let model = val
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.default_model)
            .to_string();

        let tool_calls_opt = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        Ok(LlmResponse {
            content,
            usage,
            model,
            finish_reason,
            tool_calls: tool_calls_opt,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/messages", self.base_url);
        let payload = self.serialize_request(req, false);

        let res = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let body_json = res
            .json::<serde_json::Value>()
            .await
            .map_err(|e| LlmError::Parser(format!("Failed to parse response JSON: {}", e)))?;

        self.parse_response(body_json)
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        let url = format!("{}/messages", self.base_url);
        let payload = self.serialize_request(req, true);

        let res = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| LlmError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(LlmError::Http {
                status: status.as_u16(),
                body,
            });
        }

        let bytes_stream = res.bytes_stream();
        let parse_line = |line: &str| -> Option<Result<LlmDelta, LlmError>> {
            if !line.starts_with("data: ") {
                return None;
            }
            let data = &line["data: ".len()..];
            let val = serde_json::from_str::<serde_json::Value>(data).ok()?;
            let sse_type = val.get("type")?.as_str()?;

            match sse_type {
                "content_block_delta" => {
                    let delta_val = val.get("delta")?;
                    let text = delta_val.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    Some(Ok(LlmDelta {
                        content: text,
                        tool_calls: None,
                        finish_reason: None,
                    }))
                }
                "message_delta" => {
                    let delta_val = val.get("delta")?;
                    let stop_reason = delta_val.get("stop_reason").and_then(|s| s.as_str());
                    let finish_reason = stop_reason.map(|sr| match sr {
                        "end_turn" => FinishReason::Stop,
                        "max_tokens" => FinishReason::Length,
                        "tool_use" => FinishReason::ToolCalls,
                        other => FinishReason::Other(other.to_string()),
                    });
                    Some(Ok(LlmDelta {
                        content: String::new(),
                        tool_calls: None,
                        finish_reason,
                    }))
                }
                _ => None, // Ignore other events like Ping, message_start, etc.
            }
        };

        Ok(create_sse_stream(bytes_stream, parse_line))
    }
}

// =========================================================================
// 4. Grok Provider (xAI Integration - OpenAI Compatible Wrapper)
// =========================================================================

pub struct GrokProvider {
    pub inner: OpenAIProvider,
}

impl GrokProvider {
    pub fn new(api_key: String, base_url: Option<String>, default_model: Option<String>) -> Self {
        Self {
            inner: OpenAIProvider::new(
                api_key,
                Some(base_url.unwrap_or_else(|| "https://api.x.ai/v1".to_string())),
                Some(default_model.unwrap_or_else(|| "grok-2".to_string())),
            ),
        }
    }
}

#[async_trait]
impl LlmProvider for GrokProvider {
    fn name(&self) -> &'static str {
        "grok"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.inner.complete(req).await
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        self.inner.complete_stream(req).await
    }
}

// =========================================================================
// 5. Local Ollama Provider (OpenAI Compatible Localhost Adapter)
// =========================================================================

pub struct LocalOllamaProvider {
    pub inner: OpenAIProvider,
}

impl LocalOllamaProvider {
    pub fn new(base_url: Option<String>, default_model: Option<String>) -> Self {
        Self {
            inner: OpenAIProvider::new(
                "ollama".to_string(), // Dummy API key, Ollama doesn't require auth
                Some(base_url.unwrap_or_else(|| "http://localhost:11434/v1".to_string())),
                Some(default_model.unwrap_or_else(|| "llama3".to_string())),
            ),
        }
    }
}

#[async_trait]
impl LlmProvider for LocalOllamaProvider {
    fn name(&self) -> &'static str {
        "ollama"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.inner.complete(req).await
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        self.inner.complete_stream(req).await
    }
}

// =========================================================================
// Resilient Decorator (Retry with Exponential Backoff & Circuit Breaker)
// =========================================================================

pub struct CircuitBreaker {
    failures_threshold: usize,
    cooldown_period: Duration,
    state: Mutex<BreakerState>,
}

struct BreakerState {
    consecutive_failures: usize,
    tripped_at: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(failures_threshold: usize, cooldown_period: Duration) -> Self {
        Self {
            failures_threshold,
            cooldown_period,
            state: Mutex::new(BreakerState {
                consecutive_failures: 0,
                tripped_at: None,
            }),
        }
    }

    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if let Some(tripped_time) = state.tripped_at {
            if tripped_time.elapsed() > self.cooldown_period {
                // Cooldown passed, transition to half-open state by allowing single trial
                state.tripped_at = None;
                true
            } else {
                false // Breaker is open
            }
        } else {
            true // Breaker is closed
        }
    }

    pub fn record_success(&self) {
        let mut state = self.state.lock().unwrap();
        state.consecutive_failures = 0;
        state.tripped_at = None;
    }

    pub fn record_failure(&self) {
        let mut state = self.state.lock().unwrap();
        state.consecutive_failures += 1;
        if state.consecutive_failures >= self.failures_threshold {
            state.tripped_at = Some(Instant::now());
        }
    }
}

pub struct ResilientLlmProvider {
    pub inner: Arc<dyn LlmProvider>,
    pub max_retries: usize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub circuit_breaker: Arc<CircuitBreaker>,
}

impl ResilientLlmProvider {
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        Self {
            inner,
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(5),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(10))),
        }
    }
}

#[async_trait]
impl LlmProvider for ResilientLlmProvider {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        if !self.circuit_breaker.allow_request() {
            return Err(LlmError::CircuitBreakerOpen);
        }

        let mut delay = self.initial_delay;
        let mut last_err = LlmError::Unknown("No attempts made".to_string());

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, self.max_delay);
            }

            match self.inner.complete(req.clone()).await {
                Ok(resp) => {
                    self.circuit_breaker.record_success();
                    return Ok(resp);
                }
                Err(err) => {
                    last_err = err;
                    self.circuit_breaker.record_failure();
                }
            }
        }

        Err(last_err)
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        if !self.circuit_breaker.allow_request() {
            return Err(LlmError::CircuitBreakerOpen);
        }

        // Streaming is not retried midway, we only retry the initial request establishment
        let mut delay = self.initial_delay;
        let mut last_err = LlmError::Unknown("No attempts made".to_string());

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, self.max_delay);
            }

            match self.inner.complete_stream(req.clone()).await {
                Ok(stream) => {
                    self.circuit_breaker.record_success();
                    return Ok(stream);
                }
                Err(err) => {
                    last_err = err;
                    self.circuit_breaker.record_failure();
                }
            }
        }

        Err(last_err)
    }
}

// =========================================================================
// Factory and Config Loading
// =========================================================================

pub struct KorgConfig {
    pub default_llm: String,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub grok_api_key: Option<String>,
    pub grok_base_url: Option<String>,
    pub ollama_base_url: Option<String>,
    pub default_model: Option<String>,
}

impl KorgConfig {
    pub fn from_env() -> Self {
        Self {
            default_llm: std::env::var("KORG_DEFAULT_LLM").unwrap_or_else(|_| "mock".to_string()),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            anthropic_base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
            grok_api_key: std::env::var("GROK_API_KEY").ok(),
            grok_base_url: std::env::var("GROK_BASE_URL").ok(),
            ollama_base_url: std::env::var("OLLAMA_BASE_URL").ok(),
            default_model: std::env::var("KORG_MODEL").ok(),
        }
    }
}

pub fn build_provider(config: &KorgConfig) -> Arc<dyn LlmProvider> {
    let raw_provider: Arc<dyn LlmProvider> = match config.default_llm.as_str() {
        "openai" => {
            let key = config.openai_api_key.clone().unwrap_or_else(|| "mock-key".to_string());
            Arc::new(OpenAIProvider::new(key, config.openai_base_url.clone(), config.default_model.clone()))
        }
        "anthropic" => {
            let key = config.anthropic_api_key.clone().unwrap_or_else(|| "mock-key".to_string());
            Arc::new(AnthropicProvider::new(key, config.anthropic_base_url.clone(), config.default_model.clone()))
        }
        "grok" => {
            let key = config.grok_api_key.clone().unwrap_or_else(|| "mock-key".to_string());
            Arc::new(GrokProvider::new(key, config.grok_base_url.clone(), config.default_model.clone()))
        }
        "ollama" => {
            Arc::new(LocalOllamaProvider::new(config.ollama_base_url.clone(), config.default_model.clone()))
        }
        _ => Arc::new(MockProvider::new()),
    };

    Arc::new(ResilientLlmProvider::new(raw_provider))
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider_completes_offline() {
        let provider = MockProvider::new();
        let request = LlmRequest {
            messages: vec![Message {
                role: Role::User,
                content: "Evaluate transaction authenticity".to_string(),
                name: None,
                tool_calls: None,
            }],
            temperature: 0.2,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
        };

        let response = provider.complete(request).await.unwrap();
        assert!(response.content.contains("Evaluate transaction authenticity"));
        assert_eq!(response.model, "mock-model-v1");
    }

    #[test]
    fn test_openai_payload_serialization() {
        let provider = OpenAIProvider::new(
            "dummy_key".to_string(),
            None,
            Some("gpt-4o-mini".to_string()),
        );

        let request = LlmRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: "System instructions".to_string(),
                    name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: "Implement transaction validator".to_string(),
                    name: None,
                    tool_calls: None,
                },
            ],
            temperature: 0.5,
            max_tokens: Some(512),
            tools: Some(vec![ToolDefinition {
                name: "git_diff".to_string(),
                description: "Applies a unified git diff".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "diff": { "type": "string" }
                    }
                }),
            }]),
            stop_sequences: Some(vec!["\n".to_string()]),
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
        };

        let payload = provider.serialize_request(request, false);

        assert_eq!(payload["model"], "gpt-4o-mini");
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["max_tokens"], 512);
        assert_eq!(payload["stop"], serde_json::json!(vec!["\n"]));
        assert!(payload["tools"].is_array());
        assert_eq!(payload["tools"][0]["function"]["name"], "git_diff");
        assert_eq!(payload["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_anthropic_payload_serialization() {
        let provider = AnthropicProvider::new(
            "dummy_key".to_string(),
            None,
            Some("claude-3-opus-20240229".to_string()),
        );

        let request = LlmRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: "Act as an evaluator persona".to_string(),
                    name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: "Verify CRDT blackboard locks".to_string(),
                    name: None,
                    tool_calls: None,
                },
            ],
            temperature: 0.5,
            max_tokens: Some(1024),
            tools: Some(vec![ToolDefinition {
                name: "read_lock".to_string(),
                description: "Reads locks".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "lock_id": { "type": "string" }
                    }
                }),
            }]),
            stop_sequences: Some(vec!["DONE".to_string()]),
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
        };

        let payload = provider.serialize_request(request, false);

        assert_eq!(payload["model"], "claude-3-opus-20240229");
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["max_tokens"], 1024);
        assert_eq!(payload["system"], "Act as an evaluator persona");
        assert_eq!(payload["stop_sequences"], serde_json::json!(vec!["DONE"]));
        
        // Assert system prompt was extracted and only 1 user message remains in messages array
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Verify CRDT blackboard locks");

        // Assert tool format (parameters -> input_schema)
        assert!(payload["tools"].is_array());
        assert_eq!(payload["tools"][0]["name"], "read_lock");
        assert!(payload["tools"][0]["input_schema"].is_object());
    }

    #[tokio::test]
    async fn test_retry_backoff_logic() {
        let mock = Arc::new(MockProvider::new());
        mock.set_response(Err(LlmError::RateLimit("API key throttled".to_string())));
        mock.set_response(Err(LlmError::RateLimit("API key throttled".to_string())));
        mock.set_response(Err(LlmError::RateLimit("API key throttled".to_string())));

        let resilient = ResilientLlmProvider {
            inner: mock.clone(),
            max_retries: 2,
            initial_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(50),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(1))),
        };

        let request = LlmRequest {
            messages: vec![],
            temperature: 0.0,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
        };

        let res = resilient.complete(request).await;
        assert!(res.is_err());
        match res.unwrap_err() {
            LlmError::RateLimit(msg) => assert_eq!(msg, "API key throttled"),
            other => panic!("Expected RateLimit error, got {:?}", other),
        }
    }
}
