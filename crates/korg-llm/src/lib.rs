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

mod deterministic;
pub use deterministic::DeterministicProvider;

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

    pub top_p: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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
            let input_summary = req
                .messages
                .last()
                .map(|m| m.content.as_str())
                .unwrap_or("");
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
                LlmDelta {
                    content: "Mock ".to_string(),
                    tool_calls: None,
                    finish_reason: None,
                },
                LlmDelta {
                    content: "stream ".to_string(),
                    tool_calls: None,
                    finish_reason: None,
                },
                LlmDelta {
                    content: "reply.".to_string(),
                    tool_calls: None,
                    finish_reason: Some(FinishReason::Stop),
                },
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
        if let Some(tp) = req.top_p {
            body["top_p"] = serde_json::json!(tp);
        }
        if let Some(pp) = req.presence_penalty {
            body["presence_penalty"] = serde_json::json!(pp);
        }
        if let Some(fp) = req.frequency_penalty {
            body["frequency_penalty"] = serde_json::json!(fp);
        }

        body
    }

    fn parse_response(&self, val: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let choice = val
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| {
                LlmError::Parser("No choices returned in OpenAI response".to_string())
            })?;

        let message = choice
            .get("message")
            .ok_or_else(|| LlmError::Parser("No message returned in choice".to_string()))?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| serde_json::from_value::<Vec<ToolCall>>(tc.clone()).ok());

        let finish_reason_str = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("stop");
        let finish_reason = match finish_reason_str {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            other => FinishReason::Other(other.to_string()),
        };

        let usage = val
            .get("usage")
            .and_then(|u| serde_json::from_value::<TokenUsage>(u.clone()).ok())
            .unwrap_or(TokenUsage {
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
                    let content = delta
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_calls = delta
                        .get("tool_calls")
                        .and_then(|tc| serde_json::from_value::<Vec<ToolCall>>(tc.clone()).ok());
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
                Err(e) => Some(Err(LlmError::Parser(format!(
                    "Failed to parse SSE line: {}",
                    e
                )))),
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
            default_model: default_model
                .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string()),
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
        if let Some(tp) = req.top_p {
            body["top_p"] = serde_json::json!(tp);
        }

        body
    }

    fn parse_response(&self, val: serde_json::Value) -> Result<LlmResponse, LlmError> {
        let content_blocks = val
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                LlmError::Parser("No content blocks returned from Anthropic".to_string())
            })?;

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

        let stop_reason_str = val
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("end_turn");
        let finish_reason = match stop_reason_str {
            "end_turn" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            other => FinishReason::Other(other.to_string()),
        };

        // Usage mapping
        let usage_val = val.get("usage");
        let input_tokens = usage_val
            .and_then(|u| u.get("input_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = usage_val
            .and_then(|u| u.get("output_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0) as u32;
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

        let tool_calls_opt = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

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
                    let text = delta_val
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
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
// Free-Tier LLM Rotator Layer
// =========================================================================

static ROTATOR_COOLDOWNS_MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

fn get_cooldowns_mutex() -> &'static Mutex<()> {
    ROTATOR_COOLDOWNS_MUTEX.get_or_init(|| Mutex::new(()))
}

fn read_persisted_cooldowns() -> std::collections::HashMap<String, chrono::DateTime<chrono::Utc>> {
    use fs2::FileExt;
    use std::io::Read;

    let proj_root = korg_core::paths::project_root();
    let korg_dir = proj_root.join(".korg");
    let _ = std::fs::create_dir_all(&korg_dir);
    let file_path = korg_dir.join("rotator_cooldowns.json");
    let lock_path = korg_dir.join("rotator_cooldowns.lock");

    // Open lock file
    let lock_file = match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return std::collections::HashMap::new(),
    };

    // Shared lock for reading
    if lock_file.lock_shared().is_err() {
        return std::collections::HashMap::new();
    }

    let mut result: std::collections::HashMap<String, chrono::DateTime<chrono::Utc>> =
        std::collections::HashMap::new();
    if file_path.exists() {
        if let Ok(mut f) = std::fs::File::open(&file_path) {
            let mut content = String::new();
            if f.read_to_string(&mut content).is_ok() {
                if let Ok(parsed) = serde_json::from_str::<
                    std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
                >(&content)
                {
                    result = parsed;
                }
            }
        }
    }

    let _ = lock_file.unlock();

    // 3. Expiration sweep on read & clock drift clamping
    let now = chrono::Utc::now();
    let max_future = now + chrono::Duration::seconds(60);
    result.retain(|_, &mut until| {
        // Sweep if already expired
        if until <= now {
            return false;
        }
        // Clamp clock drift if it is set way in the future
        if until > max_future {
            return false;
        }
        true
    });

    result
}

fn write_persisted_cooldowns(
    cooldowns: &std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
) {
    use fs2::FileExt;
    use std::io::Write;

    let proj_root = korg_core::paths::project_root();
    let korg_dir = proj_root.join(".korg");
    let _ = std::fs::create_dir_all(&korg_dir);
    let file_path = korg_dir.join("rotator_cooldowns.json");
    let tmp_path = korg_dir.join("rotator_cooldowns.tmp");
    let lock_path = korg_dir.join("rotator_cooldowns.lock");

    // Open lock file
    let lock_file = match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    // Exclusive lock for writing
    if lock_file.lock_exclusive().is_err() {
        return;
    }

    if let Ok(content) = serde_json::to_string_pretty(cooldowns) {
        // Write to tmp path first (atomic write)
        if let Ok(mut tmp_file) = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
        {
            if tmp_file.write_all(content.as_bytes()).is_ok() {
                let _ = tmp_file.sync_all(); // Hard fsync call
                drop(tmp_file);
                // Atomic rename
                let _ = std::fs::rename(&tmp_path, &file_path);
            }
        }
    }

    let _ = lock_file.unlock();
}

pub struct RotatorCandidateState {
    pub name: String,
    pub provider: Arc<dyn LlmProvider>,
    pub default_model: Option<String>,
}

pub struct RotatorProvider {
    pub candidates: Vec<RotatorCandidateState>,
}

impl RotatorProvider {
    pub fn new(candidates: Vec<RotatorCandidateState>) -> Self {
        Self { candidates }
    }

    /// Selects the best candidate to try.
    /// Returns the index of the selected candidate.
    fn select_candidate(&self) -> Option<usize> {
        if self.candidates.is_empty() {
            return None;
        }

        let now = chrono::Utc::now();
        let _guard = get_cooldowns_mutex().lock().unwrap();
        let registry = read_persisted_cooldowns();

        // 1. First, try to find a candidate not on cooldown.
        for (i, cand) in self.candidates.iter().enumerate() {
            match registry.get(&cand.name) {
                None => return Some(i),
                Some(&until) if now >= until => {
                    // Cooldown has expired!
                    return Some(i);
                }
                _ => {}
            }
        }

        // 2. If all candidates are on cooldown, fall back to the one whose cooldown expires first.
        let mut best_idx = 0;
        let mut min_remaining = chrono::Duration::seconds(999999);

        for (i, cand) in self.candidates.iter().enumerate() {
            if let Some(&until) = registry.get(&cand.name) {
                if until > now {
                    let remaining = until.signed_duration_since(now);
                    if remaining < min_remaining {
                        min_remaining = remaining;
                        best_idx = i;
                    }
                } else {
                    return Some(i);
                }
            } else {
                return Some(i);
            }
        }

        Some(best_idx)
    }

    /// Place a candidate on a 60-second cooldown due to failure.
    fn trigger_cooldown(&self, idx: usize) {
        if let Some(cand) = self.candidates.get(idx) {
            let cooldown_dur = chrono::Duration::seconds(60);
            let _guard = get_cooldowns_mutex().lock().unwrap();
            let mut registry = read_persisted_cooldowns();
            registry.insert(cand.name.clone(), chrono::Utc::now() + cooldown_dur);
            write_persisted_cooldowns(&registry);

            // Console warning in dynamic colors
            let gold = "\x1b[38;2;255;215;0m";
            let reset = "\x1b[0m";
            eprintln!(
                "{gold}⚠️  [rotator] Candidate '{}' encountered a transient failure. Placing on cooldown for 60 seconds.{reset}",
                cand.name
            );
        }
    }
}

#[async_trait]
impl LlmProvider for RotatorProvider {
    fn name(&self) -> &'static str {
        "rotator"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        if self.candidates.is_empty() {
            return Err(LlmError::Unknown(
                "No rotator candidates configured".to_string(),
            ));
        }

        let mut attempted = std::collections::HashSet::new();

        loop {
            if attempted.len() >= self.candidates.len() {
                break;
            }

            let idx = self.select_candidate().unwrap_or(0);
            let mut selected_idx = idx;
            if attempted.contains(&selected_idx) {
                let mut found = false;
                for i in 0..self.candidates.len() {
                    if !attempted.contains(&i) {
                        selected_idx = i;
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
            }

            attempted.insert(selected_idx);
            let candidate = &self.candidates[selected_idx];

            let mut request = req.clone();
            if let Some(ref custom_model) = candidate.default_model {
                // If the candidate has a customized default model (e.g. from the rotator),
                // it is already hardcoded inside the candidate's provider if it's OpenAIProvider.
                // But in case the request is direct or other providers, we let it carry.
            }

            match candidate.provider.complete(request).await {
                Ok(resp) => {
                    ROTATOR_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(resp);
                }
                Err(err) => {
                    self.trigger_cooldown(selected_idx);
                }
            }
        }

        Err(LlmError::Unknown(
            "All rotator candidates failed".to_string(),
        ))
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        if self.candidates.is_empty() {
            return Err(LlmError::Unknown(
                "No rotator candidates configured".to_string(),
            ));
        }

        let mut attempted = std::collections::HashSet::new();

        loop {
            if attempted.len() >= self.candidates.len() {
                break;
            }

            let idx = self.select_candidate().unwrap_or(0);
            let mut selected_idx = idx;
            if attempted.contains(&selected_idx) {
                let mut found = false;
                for i in 0..self.candidates.len() {
                    if !attempted.contains(&i) {
                        selected_idx = i;
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
            }

            attempted.insert(selected_idx);
            let candidate = &self.candidates[selected_idx];

            match candidate.provider.complete_stream(req.clone()).await {
                Ok(stream) => {
                    ROTATOR_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Ok(stream);
                }
                Err(_err) => {
                    self.trigger_cooldown(selected_idx);
                }
            }
        }

        Err(LlmError::Unknown(
            "All rotator candidates failed to establish stream".to_string(),
        ))
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

pub struct SemanticLlmCache;

impl SemanticLlmCache {
    fn cache_path() -> std::path::PathBuf {
        korg_core::paths::project_root()
            .join(".korg")
            .join("semantic_llm_cache.json")
    }

    fn lock_path() -> std::path::PathBuf {
        korg_core::paths::project_root()
            .join(".korg")
            .join("semantic_cache.lock")
    }

    pub fn get(req: &LlmRequest) -> Option<LlmResponse> {
        let key_hash = match Self::compute_hash(req) {
            Ok(h) => h,
            Err(_) => return None,
        };

        let lock_path = Self::lock_path();
        let _ = std::fs::create_dir_all(lock_path.parent().unwrap());

        let lock_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
            .ok()?;

        use fs2::FileExt;
        if lock_file.lock_shared().is_ok() {
            let cache_file_path = Self::cache_path();
            let mut cache: std::collections::HashMap<String, LlmResponse> =
                std::collections::HashMap::new();
            if cache_file_path.exists() {
                if let Ok(mut f) = std::fs::File::open(&cache_file_path) {
                    let mut content = String::new();
                    use std::io::Read;
                    if f.read_to_string(&mut content).is_ok() {
                        if let Ok(parsed) = serde_json::from_str(&content) {
                            cache = parsed;
                        }
                    }
                }
            }
            let _ = lock_file.unlock();
            cache.get(&key_hash).cloned()
        } else {
            None
        }
    }

    pub fn insert(req: &LlmRequest, resp: &LlmResponse) {
        let key_hash = match Self::compute_hash(req) {
            Ok(h) => h,
            Err(_) => return,
        };

        let lock_path = Self::lock_path();
        let _ = std::fs::create_dir_all(lock_path.parent().unwrap());

        let lock_file = match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&lock_path)
        {
            Ok(f) => f,
            Err(_) => return,
        };

        use fs2::FileExt;
        if lock_file.lock_exclusive().is_ok() {
            let cache_file_path = Self::cache_path();
            let tmp_path = cache_file_path.with_extension("tmp");

            let mut cache: std::collections::HashMap<String, LlmResponse> =
                std::collections::HashMap::new();
            if cache_file_path.exists() {
                if let Ok(mut f) = std::fs::File::open(&cache_file_path) {
                    let mut content = String::new();
                    use std::io::Read;
                    if f.read_to_string(&mut content).is_ok() {
                        if let Ok(parsed) = serde_json::from_str(&content) {
                            cache = parsed;
                        }
                    }
                }
            }

            cache.insert(key_hash, resp.clone());

            if let Ok(content) = serde_json::to_string_pretty(&cache) {
                if let Ok(mut tmp_file) = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&tmp_path)
                {
                    use std::io::Write;
                    if tmp_file.write_all(content.as_bytes()).is_ok() {
                        if tmp_file.sync_all().is_ok() {
                            let _ = std::fs::rename(&tmp_path, &cache_file_path);
                        }
                    }
                }
            }
            let _ = lock_file.unlock();
        }
    }

    fn compute_hash(req: &LlmRequest) -> Result<String, serde_json::Error> {
        use sha2::Digest;
        let key_val = serde_json::json!({
            "messages": req.messages,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "tools": req.tools,
            "stop_sequences": req.stop_sequences,
            "top_p": req.top_p,
            "presence_penalty": req.presence_penalty,
            "frequency_penalty": req.frequency_penalty,
        });
        let serialized = serde_json::to_vec(&key_val)?;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&serialized);
        Ok(hex::encode(hasher.finalize()))
    }
}

pub static CAMPAIGN_TOKENS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static ROTATOR_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static HEALS_RESOLVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static COMPLETIONS_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
pub static TOTAL_LATENCY_MS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub struct ResilientLlmProvider {
    pub inner: Arc<dyn LlmProvider>,
    pub max_retries: usize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub circuit_breaker: Arc<CircuitBreaker>,
}

impl ResilientLlmProvider {
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        let config = KorgConfig::load();
        Self {
            inner,
            max_retries: config.resilience.max_retries as usize,
            initial_delay: Duration::from_millis(config.resilience.initial_delay_ms),
            max_delay: Duration::from_millis(config.resilience.max_delay_ms),
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

        let config = KorgConfig::load();
        if config.resilience.enable_semantic_cache {
            if let Some(cached) = SemanticLlmCache::get(&req) {
                let gold = "\x1b[38;2;255;215;0m";
                let reset = "\x1b[0m";
                println!("{gold}⚡ [semantic-cache] Bypassing LLM call. Returning cached response.{reset}");
                return Ok(cached);
            }
        }

        let current_campaign_tokens = CAMPAIGN_TOKENS.load(std::sync::atomic::Ordering::Relaxed);
        if current_campaign_tokens >= config.security_tokens.max_campaign_tokens as usize {
            let gold = "\x1b[38;2;255;215;0m";
            let reset = "\x1b[0m";
            eprintln!("{gold}⚠️  [security-tokens] Campaign budget limit of {} exceeded (currently {}). Halting further operations.{reset}", config.security_tokens.max_campaign_tokens, current_campaign_tokens);
            return Err(LlmError::RateLimit(format!(
                "Campaign token limit of {} exceeded (currently {})",
                config.security_tokens.max_campaign_tokens, current_campaign_tokens
            )));
        }

        let mut delay = self.initial_delay;
        let mut last_err = LlmError::Unknown("No attempts made".to_string());

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, self.max_delay);
            }

            let start_inst = std::time::Instant::now();
            match self.inner.complete(req.clone()).await {
                Ok(resp) => {
                    let elapsed_ms = start_inst.elapsed().as_millis() as usize;
                    COMPLETIONS_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    TOTAL_LATENCY_MS.fetch_add(elapsed_ms, std::sync::atomic::Ordering::Relaxed);

                    self.circuit_breaker.record_success();

                    if config.resilience.enable_semantic_cache {
                        SemanticLlmCache::insert(&req, &resp);
                    }

                    let usage_total = resp.usage.total_tokens;
                    if usage_total > config.security_tokens.max_request_tokens {
                        let gold = "\x1b[38;2;255;215;0m";
                        let reset = "\x1b[0m";
                        eprintln!("{gold}⚠️  [security-tokens] Single request token limit of {} exceeded (got {}).{reset}", config.security_tokens.max_request_tokens, usage_total);
                        return Err(LlmError::RateLimit(format!(
                            "Request token limit of {} exceeded (got {})",
                            config.security_tokens.max_request_tokens, usage_total
                        )));
                    }

                    let prev = CAMPAIGN_TOKENS
                        .fetch_add(usage_total as usize, std::sync::atomic::Ordering::Relaxed);
                    let new_total = prev + usage_total as usize;
                    let limit = config.security_tokens.max_campaign_tokens as usize;
                    if new_total >= (limit * 8 / 10) && prev < (limit * 8 / 10) {
                        let gold = "\x1b[38;2;255;215;0m";
                        let reset = "\x1b[0m";
                        println!("{gold}⚠️  [security-tokens] Campaign token usage has reached 80% of budget ({} / {}).{reset}", new_total, limit);
                    }

                    if new_total >= limit {
                        let gold = "\x1b[38;2;255;215;0m";
                        let reset = "\x1b[0m";
                        eprintln!("{gold}⚠️  [security-tokens] Campaign budget limit of {} exceeded (currently {}). Halting further operations.{reset}", limit, new_total);
                        return Err(LlmError::RateLimit(format!(
                            "Campaign token limit of {} exceeded (currently {})",
                            limit, new_total
                        )));
                    }

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

#[derive(Clone, Debug, Deserialize)]
pub struct TomlLlmSection {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlOllamaSection {
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VisionPolicyConfig {
    pub allow_raw_screenshots: bool,
    pub redact_before_broadcast: bool,
    pub block_patterns: Vec<String>,
    pub redaction_mode: String,
    pub operator_override_allowed: bool,
}

impl Default for VisionPolicyConfig {
    fn default() -> Self {
        Self {
            allow_raw_screenshots: false,
            redact_before_broadcast: true,
            block_patterns: vec![
                "password".to_string(),
                "bearer ".to_string(),
                "token=".to_string(),
                "api_key".to_string(),
                "secret".to_string(),
                "private_key".to_string(),
            ],
            redaction_mode: "blackout".to_string(),
            operator_override_allowed: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathsPolicyConfig {
    pub allowed_directories: Vec<String>,
    pub blocked_paths: Vec<String>,
}

impl Default for PathsPolicyConfig {
    fn default() -> Self {
        Self {
            allowed_directories: vec![
                korg_core::paths::project_root_string(),
                korg_core::paths::cache_dir().display().to_string(),
                "/tmp".to_string(),
            ],
            blocked_paths: vec![
                "/etc".to_string(),
                "~/.ssh".to_string(),
                ".env".to_string(),
                ".git".to_string(),
                "/etc/passwd".to_string(),
                "/etc/shadow".to_string(),
                "id_rsa".to_string(),
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkPolicyConfig {
    pub allowed_domains: Vec<String>,
    pub blocked_domains: Vec<String>,
}

impl Default for NetworkPolicyConfig {
    fn default() -> Self {
        Self {
            allowed_domains: vec![
                "github.com".to_string(),
                "crates.io".to_string(),
                "api.github.com".to_string(),
                "localhost".to_string(),
            ],
            blocked_domains: vec![
                "evil.com".to_string(),
                "malicious-subnet.net".to_string(),
                "10.0.0.1".to_string(),
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokensPolicyConfig {
    pub max_request_tokens: u32,
    pub max_campaign_tokens: u32,
}

impl Default for TokensPolicyConfig {
    fn default() -> Self {
        Self {
            max_request_tokens: 50000,
            max_campaign_tokens: 1000000,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlSecuritySection {
    pub policies: Option<TomlPoliciesSection>,
    pub allow_unsafe_commands: Option<bool>,
    pub sandbox_mode: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlPoliciesSection {
    pub vision: Option<TomlVisionPolicySection>,
    pub paths: Option<TomlPathsPolicySection>,
    pub network: Option<TomlNetworkPolicySection>,
    pub tokens: Option<TomlTokensPolicySection>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlPathsPolicySection {
    pub allowed_directories: Option<Vec<String>>,
    pub blocked_paths: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlNetworkPolicySection {
    pub allowed_domains: Option<Vec<String>>,
    pub blocked_domains: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlTokensPolicySection {
    pub max_request_tokens: Option<u32>,
    pub max_campaign_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlVisionPolicySection {
    pub allow_raw_screenshots: Option<bool>,
    pub redact_before_broadcast: Option<bool>,
    pub block_patterns: Option<Vec<String>>,
    pub redaction_mode: Option<String>,
    pub operator_override_allowed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlRotatorCandidate {
    pub name: String,
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlRotatorSection {
    pub candidates: Option<Vec<TomlRotatorCandidate>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlResilienceSection {
    pub enable_semantic_cache: Option<bool>,
    pub max_retries: Option<u32>,
    pub initial_delay_ms: Option<u64>,
    pub max_delay_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlConfig {
    pub default_llm: Option<String>,
    pub default_model: Option<String>,
    pub openai: Option<TomlLlmSection>,
    pub anthropic: Option<TomlLlmSection>,
    pub grok: Option<TomlLlmSection>,
    pub ollama: Option<TomlOllamaSection>,
    pub security: Option<TomlSecuritySection>,
    pub personas: Option<TomlPersonasSection>,
    pub rotator: Option<TomlRotatorSection>,
    pub resilience: Option<TomlResilienceSection>,
}

/// Per-persona LLM overrides.
///
/// Example korg.toml:
/// ```toml
/// [personas.captain]
/// provider = "openai"
/// model = "o3"               # Deep reasoning for planning & decomposition
/// temperature = 0.2
/// ```
#[derive(Clone, Debug, Deserialize, Default)]
pub struct TomlPersonasSection {
    pub captain: Option<TomlPersonaOverride>,
    pub harper: Option<TomlPersonaOverride>,
    pub benjamin: Option<TomlPersonaOverride>,
    pub lucas: Option<TomlPersonaOverride>,
    pub evaluator: Option<TomlPersonaOverride>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TomlPersonaOverride {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Per-persona provider + model override (resolved at runtime).
#[derive(Clone, Debug, Default)]
pub struct PersonaLlmOverride {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ResilienceConfig {
    pub enable_semantic_cache: bool,
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for ResilienceConfig {
    fn default() -> Self {
        Self {
            enable_semantic_cache: false,
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 5000,
        }
    }
}

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
    pub security_vision: VisionPolicyConfig,
    pub security_paths: PathsPolicyConfig,
    pub security_network: NetworkPolicyConfig,
    pub security_tokens: TokensPolicyConfig,
    pub allow_unsafe_commands: bool,
    pub sandbox_mode: String,
    /// Per-persona provider/model overrides (keyed by lowercase persona name).
    pub persona_overrides: std::collections::HashMap<String, PersonaLlmOverride>,
    pub rotator_candidates: Vec<TomlRotatorCandidate>,
    pub resilience: ResilienceConfig,
}

impl KorgConfig {
    pub fn from_env() -> Self {
        Self {
            default_llm: std::env::var("KORG_DEFAULT_LLM")
                .unwrap_or_else(|_| "deterministic".to_string()),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            openai_base_url: std::env::var("OPENAI_BASE_URL").ok(),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            anthropic_base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
            grok_api_key: std::env::var("GROK_API_KEY").ok(),
            grok_base_url: std::env::var("GROK_BASE_URL").ok(),
            ollama_base_url: std::env::var("OLLAMA_BASE_URL").ok(),
            default_model: std::env::var("KORG_MODEL").ok(),
            security_vision: VisionPolicyConfig::default(),
            security_paths: PathsPolicyConfig::default(),
            security_network: NetworkPolicyConfig::default(),
            security_tokens: TokensPolicyConfig::default(),
            allow_unsafe_commands: std::env::var("KORG_ALLOW_UNSAFE_COMMANDS")
                .map(|v| v == "true")
                .unwrap_or(false),
            sandbox_mode: std::env::var("KORG_SANDBOX_MODE")
                .unwrap_or_else(|_| "strict".to_string()),
            persona_overrides: std::collections::HashMap::new(),
            rotator_candidates: Vec::new(),
            resilience: ResilienceConfig::default(),
        }
    }

    pub fn load() -> Self {
        let mut default_llm = std::env::var("KORG_DEFAULT_LLM").ok();
        let mut default_model = std::env::var("KORG_MODEL").ok();
        let mut openai_api_key = std::env::var("OPENAI_API_KEY").ok();
        let mut openai_base_url = std::env::var("OPENAI_BASE_URL").ok();
        let mut anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let mut anthropic_base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
        let mut grok_api_key = std::env::var("GROK_API_KEY").ok();
        let mut grok_base_url = std::env::var("GROK_BASE_URL").ok();
        let mut ollama_base_url = std::env::var("OLLAMA_BASE_URL").ok();
        let mut security_vision = VisionPolicyConfig::default();
        let mut security_paths = PathsPolicyConfig::default();
        let mut security_network = NetworkPolicyConfig::default();
        let mut security_tokens = TokensPolicyConfig::default();
        let mut allow_unsafe_commands = std::env::var("KORG_ALLOW_UNSAFE_COMMANDS")
            .map(|v| v == "true")
            .ok();
        let mut sandbox_mode = std::env::var("KORG_SANDBOX_MODE").ok();

        let mut persona_overrides = std::collections::HashMap::new();
        let mut rotator_candidates = Vec::new();
        let mut resilience = ResilienceConfig::default();

        let mut toml_content = None;
        if std::path::Path::new("korg.toml").exists() {
            if let Ok(c) = std::fs::read_to_string("korg.toml") {
                toml_content = Some(c);
            }
        } else if let Some(proj_dirs) = directories::ProjectDirs::from("lol", "yvaehkorg", "korg") {
            let config_path = proj_dirs.config_dir().join("korg.toml");
            if config_path.exists() {
                if let Ok(c) = std::fs::read_to_string(config_path) {
                    toml_content = Some(c);
                }
            }
        }

        if let Some(content) = toml_content {
            if let Ok(parsed) = toml::from_str::<TomlConfig>(&content) {
                if default_llm.is_none() {
                    default_llm = parsed.default_llm;
                }
                if default_model.is_none() {
                    default_model = parsed.default_model;
                }
                if let Some(sec) = parsed.openai {
                    if openai_api_key.is_none() {
                        openai_api_key = sec.api_key;
                    }
                    if openai_base_url.is_none() {
                        openai_base_url = sec.base_url;
                    }
                }
                if let Some(sec) = parsed.anthropic {
                    if anthropic_api_key.is_none() {
                        anthropic_api_key = sec.api_key;
                    }
                    if anthropic_base_url.is_none() {
                        anthropic_base_url = sec.base_url;
                    }
                }
                if let Some(sec) = parsed.grok {
                    if grok_api_key.is_none() {
                        grok_api_key = sec.api_key;
                    }
                    if grok_base_url.is_none() {
                        grok_base_url = sec.base_url;
                    }
                }
                if let Some(sec) = parsed.ollama {
                    if ollama_base_url.is_none() {
                        ollama_base_url = sec.base_url;
                    }
                }
                // Parse per-persona overrides
                if let Some(personas) = parsed.personas {
                    let pairs: Vec<(&str, Option<TomlPersonaOverride>)> = vec![
                        ("captain", personas.captain),
                        ("harper", personas.harper),
                        ("benjamin", personas.benjamin),
                        ("lucas", personas.lucas),
                        ("evaluator", personas.evaluator),
                    ];
                    for (name, maybe_override) in pairs {
                        if let Some(ov) = maybe_override {
                            persona_overrides.insert(
                                name.to_string(),
                                PersonaLlmOverride {
                                    provider: ov.provider,
                                    model: ov.model,
                                    temperature: ov.temperature,
                                    top_p: ov.top_p,
                                    presence_penalty: ov.presence_penalty,
                                    frequency_penalty: ov.frequency_penalty,
                                    max_tokens: ov.max_tokens,
                                },
                            );
                        }
                    }
                }
                if let Some(rot) = parsed.rotator {
                    if let Some(cands) = rot.candidates {
                        rotator_candidates = cands;
                    }
                }
                if let Some(sec) = parsed.security {
                    if let Some(pols) = sec.policies {
                        if let Some(vis) = pols.vision {
                            if let Some(val) = vis.allow_raw_screenshots {
                                security_vision.allow_raw_screenshots = val;
                            }
                            if let Some(val) = vis.redact_before_broadcast {
                                security_vision.redact_before_broadcast = val;
                            }
                            if let Some(val) = vis.block_patterns {
                                security_vision.block_patterns = val;
                            }
                            if let Some(val) = vis.redaction_mode {
                                security_vision.redaction_mode = val;
                            }
                            if let Some(val) = vis.operator_override_allowed {
                                security_vision.operator_override_allowed = val;
                            }
                        }
                        if let Some(p) = pols.paths {
                            if let Some(val) = p.allowed_directories {
                                security_paths.allowed_directories = val;
                            }
                            if let Some(val) = p.blocked_paths {
                                security_paths.blocked_paths = val;
                            }
                        }
                        if let Some(n) = pols.network {
                            if let Some(val) = n.allowed_domains {
                                security_network.allowed_domains = val;
                            }
                            if let Some(val) = n.blocked_domains {
                                security_network.blocked_domains = val;
                            }
                        }
                        if let Some(t) = pols.tokens {
                            if let Some(val) = t.max_request_tokens {
                                security_tokens.max_request_tokens = val;
                            }
                            if let Some(val) = t.max_campaign_tokens {
                                security_tokens.max_campaign_tokens = val;
                            }
                        }
                    }
                    if allow_unsafe_commands.is_none() {
                        allow_unsafe_commands = sec.allow_unsafe_commands;
                    }
                    if sandbox_mode.is_none() {
                        sandbox_mode = sec.sandbox_mode;
                    }
                }
                if let Some(sec) = parsed.resilience {
                    if let Some(val) = sec.enable_semantic_cache {
                        resilience.enable_semantic_cache = val;
                    }
                    if let Some(val) = sec.max_retries {
                        resilience.max_retries = val;
                    }
                    if let Some(val) = sec.initial_delay_ms {
                        resilience.initial_delay_ms = val;
                    }
                    if let Some(val) = sec.max_delay_ms {
                        resilience.max_delay_ms = val;
                    }
                }
            }
        }

        Self {
            default_llm: default_llm.unwrap_or_else(|| "deterministic".to_string()),
            openai_api_key,
            openai_base_url,
            anthropic_api_key,
            anthropic_base_url,
            grok_api_key,
            grok_base_url,
            ollama_base_url,
            default_model,
            security_vision,
            security_paths,
            security_network,
            security_tokens,
            allow_unsafe_commands: allow_unsafe_commands.unwrap_or(false),
            sandbox_mode: sandbox_mode.unwrap_or_else(|| "strict".to_string()),
            persona_overrides,
            rotator_candidates,
            resilience,
        }
    }
}

pub fn build_provider(config: &KorgConfig) -> Arc<dyn LlmProvider> {
    build_provider_with(config, &config.default_llm, config.default_model.as_deref())
}

/// Build a provider for a specific persona, using per-persona overrides if configured.
///
/// Falls back to the default provider if no override is set for the persona.
pub fn build_provider_for_persona(
    config: &KorgConfig,
    persona_name: &str,
) -> (Arc<dyn LlmProvider>, Option<f32>) {
    let key = persona_name.to_lowercase();
    if let Some(ov) = config.persona_overrides.get(&key) {
        let provider_name = ov.provider.as_deref().unwrap_or(&config.default_llm);
        let model = ov.model.as_deref().or(config.default_model.as_deref());
        (
            build_provider_with(config, provider_name, model),
            ov.temperature,
        )
    } else {
        (build_provider(config), None)
    }
}

/// Internal: build a provider given explicit provider name and model.
fn build_provider_with(
    config: &KorgConfig,
    provider_name: &str,
    model: Option<&str>,
) -> Arc<dyn LlmProvider> {
    let model_owned = model.map(|m| m.to_string());
    let raw_provider: Arc<dyn LlmProvider> = match provider_name {
        "openai" => {
            let key = config
                .openai_api_key
                .clone()
                .unwrap_or_else(|| "mock-key".to_string());
            Arc::new(OpenAIProvider::new(
                key,
                config.openai_base_url.clone(),
                model_owned,
            ))
        }
        "anthropic" => {
            let key = config
                .anthropic_api_key
                .clone()
                .unwrap_or_else(|| "mock-key".to_string());
            Arc::new(AnthropicProvider::new(
                key,
                config.anthropic_base_url.clone(),
                model_owned,
            ))
        }
        "grok" => {
            let key = config
                .grok_api_key
                .clone()
                .unwrap_or_else(|| "mock-key".to_string());
            Arc::new(GrokProvider::new(
                key,
                config.grok_base_url.clone(),
                model_owned,
            ))
        }
        "ollama" => Arc::new(LocalOllamaProvider::new(
            config.ollama_base_url.clone(),
            model_owned,
        )),
        "rotator" => {
            let mut candidates = Vec::new();
            for cand in &config.rotator_candidates {
                let inner_provider: Arc<dyn LlmProvider> = match cand.provider.as_str() {
                    "openai" => {
                        let key = cand
                            .api_key
                            .clone()
                            .or_else(|| config.openai_api_key.clone())
                            .unwrap_or_else(|| "mock-key".to_string());
                        let base_url = cand
                            .base_url
                            .clone()
                            .or_else(|| config.openai_base_url.clone());
                        let model = cand.model.clone().or_else(|| model_owned.clone());
                        Arc::new(OpenAIProvider::new(key, base_url, model))
                    }
                    "anthropic" => {
                        let key = cand
                            .api_key
                            .clone()
                            .or_else(|| config.anthropic_api_key.clone())
                            .unwrap_or_else(|| "mock-key".to_string());
                        let base_url = cand
                            .base_url
                            .clone()
                            .or_else(|| config.anthropic_base_url.clone());
                        let model = cand.model.clone().or_else(|| model_owned.clone());
                        Arc::new(AnthropicProvider::new(key, base_url, model))
                    }
                    "grok" => {
                        let key = cand
                            .api_key
                            .clone()
                            .or_else(|| config.grok_api_key.clone())
                            .unwrap_or_else(|| "mock-key".to_string());
                        let base_url = cand
                            .base_url
                            .clone()
                            .or_else(|| config.grok_base_url.clone());
                        let model = cand.model.clone().or_else(|| model_owned.clone());
                        Arc::new(GrokProvider::new(key, base_url, model))
                    }
                    "ollama" => {
                        let base_url = cand
                            .base_url
                            .clone()
                            .or_else(|| config.ollama_base_url.clone());
                        let model = cand.model.clone().or_else(|| model_owned.clone());
                        Arc::new(LocalOllamaProvider::new(base_url, model))
                    }
                    _ => Arc::new(MockProvider::new()),
                };

                candidates.push(RotatorCandidateState {
                    name: cand.name.clone(),
                    provider: inner_provider,
                    default_model: cand.model.clone(),
                });
            }
            Arc::new(RotatorProvider::new(candidates))
        }
        "deterministic" => Arc::new(DeterministicProvider::new()),
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
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        let response = provider.complete(request).await.unwrap();
        assert!(response
            .content
            .contains("Evaluate transaction authenticity"));
        assert_eq!(response.model, "mock-model-v1");
    }

    #[tokio::test]
    async fn build_provider_default_is_deterministic_and_honest() {
        let mut cfg = KorgConfig::from_env();
        cfg.default_llm = "deterministic".to_string();
        let provider = build_provider(&cfg); // wrapped in ResilientLlmProvider
        let request = LlmRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: "You are Benjamin, the Builder & Implementer.".to_string(),
                    name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: "Implement a distributed consensus protocol".to_string(),
                    name: None,
                    tool_calls: None,
                },
            ],
            temperature: 0.3,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        };
        let resp = provider.complete(request).await.unwrap();
        // honest null for an unknown task: empty mutations, NOT the mock echo string
        assert!(
            !resp.content.contains("[Mock Response to:"),
            "default must not be the mock echo"
        );
        assert!(
            resp.content.contains("\"mutations\": []"),
            "unknown task → honest null"
        );
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
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
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
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
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
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        let res = resilient.complete(request).await;
        assert!(res.is_err());
        match res.unwrap_err() {
            LlmError::RateLimit(msg) => assert_eq!(msg, "API key throttled"),
            other => panic!("Expected RateLimit error, got {:?}", other),
        }
    }

    #[test]
    fn test_toml_config_parsing_and_defaults() {
        let toml_str = r#"
            default_llm = "grok"
            default_model = "grok-2"

            [grok]
            api_key = "grok-special-key"
            base_url = "https://api.x.ai/v1"

            [openai]
            api_key = "openai-key"

            [ollama]
            base_url = "http://localhost:11434/v1"
        "#;

        let parsed: TomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.default_llm.unwrap(), "grok");
        assert_eq!(parsed.default_model.unwrap(), "grok-2");
        assert_eq!(parsed.grok.unwrap().api_key.unwrap(), "grok-special-key");
        assert_eq!(parsed.openai.unwrap().api_key.unwrap(), "openai-key");
        assert_eq!(
            parsed.ollama.unwrap().base_url.unwrap(),
            "http://localhost:11434/v1"
        );
    }

    #[tokio::test]
    async fn test_rotator_failover_on_429() {
        let mock_fail = Arc::new(MockProvider::new());
        mock_fail.set_response(Err(LlmError::RateLimit(
            "429 Too Many Requests".to_string(),
        )));

        let mock_success = Arc::new(MockProvider::new());
        mock_success.set_response(Ok(LlmResponse {
            content: "Success response".to_string(),
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                total_tokens: 20,
            },
            model: "model-2".to_string(),
            finish_reason: FinishReason::Stop,
            tool_calls: None,
        }));

        let candidates = vec![
            RotatorCandidateState {
                name: "candidate-fail-1".to_string(),
                provider: mock_fail,
                default_model: None,
            },
            RotatorCandidateState {
                name: "candidate-success-2".to_string(),
                provider: mock_success,
                default_model: None,
            },
        ];

        let rotator = RotatorProvider::new(candidates);
        let request = LlmRequest {
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        // This should try candidate-fail-1 first, trigger a cooldown, and then try candidate-success-2 and succeed!
        let resp = rotator.complete(request).await.unwrap();
        assert_eq!(resp.content, "Success response");

        // Verify candidate-fail-1 has a cooldown
        let cooldowns = read_persisted_cooldowns();
        assert!(cooldowns.contains_key("candidate-fail-1"));
    }

    #[tokio::test]
    async fn test_rotator_cooldown_skips() {
        // Place candidate-1 on a manual cooldown
        {
            let mut cooldowns = read_persisted_cooldowns();
            cooldowns.insert(
                "candidate-cooldown-1".to_string(),
                chrono::Utc::now() + chrono::Duration::seconds(60),
            );
            write_persisted_cooldowns(&cooldowns);
        }

        let mock_cooldown = Arc::new(MockProvider::new());
        // If it gets called, fail the test
        mock_cooldown.set_response(Err(LlmError::Unknown("Should not be called!".to_string())));

        let mock_active = Arc::new(MockProvider::new());
        mock_active.set_response(Ok(LlmResponse {
            content: "Active candidate".to_string(),
            usage: TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 5,
                total_tokens: 10,
            },
            model: "model-active".to_string(),
            finish_reason: FinishReason::Stop,
            tool_calls: None,
        }));

        let candidates = vec![
            RotatorCandidateState {
                name: "candidate-cooldown-1".to_string(),
                provider: mock_cooldown,
                default_model: None,
            },
            RotatorCandidateState {
                name: "candidate-active-2".to_string(),
                provider: mock_active,
                default_model: None,
            },
        ];

        let rotator = RotatorProvider::new(candidates);
        let request = LlmRequest {
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        };

        // This should skip candidate-cooldown-1 and return success from candidate-active-2 immediately!
        let resp = rotator.complete(request).await.unwrap();
        assert_eq!(resp.content, "Active candidate");
    }

    #[test]
    fn test_semantic_llm_cache_insert_and_get() {
        let cache_file = SemanticLlmCache::cache_path();
        let backup_file = cache_file.with_extension("backup_test");

        if cache_file.exists() {
            let _ = std::fs::rename(&cache_file, &backup_file);
        }

        let request = LlmRequest {
            messages: vec![Message {
                role: Role::User,
                content: "Semantic cache test message".to_string(),
                name: None,
                tool_calls: None,
            }],
            temperature: 0.88,
            max_tokens: Some(123),
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: Some(0.99),
            presence_penalty: Some(0.12),
            frequency_penalty: Some(0.34),
        };

        let response = LlmResponse {
            content: "Cached response content".to_string(),
            usage: TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
            model: "cache-test-model".to_string(),
            finish_reason: FinishReason::Stop,
            tool_calls: None,
        };

        // Assert empty cache get returns None
        let get_before = SemanticLlmCache::get(&request);
        assert!(get_before.is_none());

        // Insert response
        SemanticLlmCache::insert(&request, &response);

        // Get response and assert equality
        let get_after = SemanticLlmCache::get(&request);
        assert!(get_after.is_some());
        let get_after_resp = get_after.unwrap();
        assert_eq!(get_after_resp.content, "Cached response content");
        assert_eq!(get_after_resp.model, "cache-test-model");

        // Cleanup test cache file and restore backup
        let _ = std::fs::remove_file(&cache_file);
        let _ = std::fs::remove_file(SemanticLlmCache::lock_path());
        if backup_file.exists() {
            let _ = std::fs::rename(&backup_file, &cache_file);
        }
    }

    #[test]
    fn test_custom_overrides_serialization() {
        let provider =
            OpenAIProvider::new("test_key".to_string(), None, Some("gpt-4o".to_string()));

        let request = LlmRequest {
            messages: vec![],
            temperature: 0.1,
            max_tokens: Some(10),
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: Some(0.85),
            presence_penalty: Some(0.45),
            frequency_penalty: Some(0.65),
        };

        let payload = provider.serialize_request(request, false);
        assert!((payload["top_p"].as_f64().unwrap() - 0.85).abs() < 1e-5);
        assert!((payload["presence_penalty"].as_f64().unwrap() - 0.45).abs() < 1e-5);
        assert!((payload["frequency_penalty"].as_f64().unwrap() - 0.65).abs() < 1e-5);
    }
}
