use async_stream::try_stream;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::stream::BoxStream;
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;
use tracing::warn;

use async_openai::{
    config::OpenAIConfig,
    types::{
        audio::{
            AudioInput, AudioResponseFormat, CreateSpeechRequestArgs,
            CreateTranscriptionRequestArgs, SpeechModel, SpeechResponseFormat, Voice,
        },
        chat::{
            ChatCompletionMessageToolCalls, ChatCompletionRequestMessage,
            ChatCompletionRequestMessageContentPartImage,
            ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessageArgs,
            ChatCompletionRequestUserMessageArgs, ChatCompletionRequestUserMessageContent,
            ChatCompletionRequestUserMessageContentPart, ChatCompletionTool, ChatCompletionTools,
            CreateChatCompletionRequestArgs, FunctionCall, FunctionObject, ImageDetail, ImageUrl,
            ResponseFormat, ResponseFormatJsonSchema,
        },
        embeddings::{CreateEmbeddingRequestArgs, EmbeddingInput},
        InputSource,
    },
    Client,
};

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::providers::{
    ChatEvent, ImageData, ImageInput, LlmProvider, LlmResponse, ToolCall,
};

enum ChatCreateResult {
    Parsed(async_openai::types::chat::CreateChatCompletionResponse),
    Raw(Value),
}

#[derive(Clone)]
pub struct OpenAiProvider {
    model: String,
    client: Client<OpenAIConfig>,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    fn is_openai_function_name(name: &str) -> bool {
        let trimmed = name.trim();
        !trimmed.is_empty()
            && trimmed
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }

    pub fn new(api_key: String, model: Option<String>, base_url: Option<String>) -> Self {
        let model = model.unwrap_or_else(|| "gpt-4.1-mini".to_string());
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        let api_key_for_config = api_key.clone();
        let base_url_for_config = base_url.clone();
        let config = OpenAIConfig::new()
            .with_api_key(api_key_for_config)
            .with_api_base(base_url_for_config);
        Self {
            model,
            client: Client::with_config(config),
            api_key,
            base_url,
        }
    }

    async fn raw_chat_completion(
        &self,
        request: &async_openai::types::chat::CreateChatCompletionRequest,
    ) -> Result<Value> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let client = reqwest::Client::new();
        for attempt in 0..3 {
            let response = client
                .post(url.clone())
                .bearer_auth(&self.api_key)
                .json(request)
                .send()
                .await
                .map_err(|e| {
                    ButterflyBotError::Http(format!("Chat completion transport failed: {e}"))
                })?;
            let status = response.status();
            let body = response.text().await.map_err(|e| {
                ButterflyBotError::Http(format!("Chat completion read failed: {e}"))
            })?;

            if status == StatusCode::OK {
                return serde_json::from_str(&body).map_err(|e| {
                    ButterflyBotError::Serialization(format!("Chat completion decode failed: {e}"))
                });
            }

            let lower = body.to_ascii_lowercase();
            let retryable_json_truncation = status.is_server_error()
                && (lower.contains("unexpected end of json input")
                    || lower.contains("unexpected end of json")
                    || lower.contains("unexpected end of input")
                    || lower.contains("unexpected eof"));

            if retryable_json_truncation && attempt < 2 {
                tokio::time::sleep(Duration::from_millis(150 * (attempt + 1) as u64)).await;
                continue;
            }

            return Err(ButterflyBotError::Http(format!(
                "Chat completion failed ({status}): {body}"
            )));
        }

        Err(ButterflyBotError::Http(
            "Chat completion failed after retries".to_string(),
        ))
    }

    async fn chat_create_with_fallback(
        &self,
        request: async_openai::types::chat::CreateChatCompletionRequest,
    ) -> Result<ChatCreateResult> {
        match self.raw_chat_completion(&request).await {
            Ok(raw) => return Ok(ChatCreateResult::Raw(raw)),
            Err(ButterflyBotError::Http(message))
                if message.starts_with("Chat completion failed (") =>
            {
                let lower = message.to_ascii_lowercase();
                if !lower.contains("unexpected end of json input")
                    && !lower.contains("unexpected end of json")
                    && !lower.contains("unexpected end of input")
                    && !lower.contains("unexpected eof")
                {
                    return Err(ButterflyBotError::Http(message));
                }
            }
            Err(_) => {}
        }

        match self.client.chat().create(request).await {
            Ok(response) => Ok(ChatCreateResult::Parsed(response)),
            Err(err) => Err(ButterflyBotError::Http(err.to_string())),
        }
    }

    fn extract_text_from_value(response: &Value) -> Option<String> {
        response
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(|content| content.as_str())
            .map(|text| text.to_string())
    }

    fn extract_tool_calls_from_value(response: &Value) -> Vec<ToolCall> {
        let message = response
            .get("choices")
            .and_then(|v| v.get(0))
            .and_then(|choice| choice.get("message"))
            .cloned()
            .unwrap_or(Value::Null);

        let Some(calls) = message.get("tool_calls").and_then(|calls| calls.as_array()) else {
            #[allow(deprecated)]
            if let Some(function_call) = message.get("function_call") {
                if let Some(name) = function_call.get("name").and_then(|value| value.as_str()) {
                    let arguments = function_call.get("arguments");
                    let arguments = match arguments {
                        Some(Value::String(text)) => {
                            serde_json::from_str(text).unwrap_or(Value::String(text.clone()))
                        }
                        Some(value) => value.clone(),
                        None => Value::Null,
                    };
                    return vec![ToolCall {
                        name: name.to_string(),
                        arguments,
                    }];
                }
            }
            return Vec::new();
        };

        calls
            .iter()
            .filter_map(|call| {
                let function = call.get("function");
                let custom_tool = call.get("custom_tool");

                let (name, arguments) = if let Some(function) = function {
                    (
                        function.get("name")?.as_str()?.to_string(),
                        function.get("arguments"),
                    )
                } else if let Some(custom_tool) = custom_tool {
                    (
                        custom_tool.get("name")?.as_str()?.to_string(),
                        custom_tool.get("input"),
                    )
                } else {
                    return None;
                };

                let arguments = match arguments {
                    Some(Value::String(text)) => {
                        serde_json::from_str(text).unwrap_or(Value::String(text.clone()))
                    }
                    Some(value) => value.clone(),
                    None => Value::Null,
                };
                Some(ToolCall { name, arguments })
            })
            .collect()
    }

    fn build_system_message(system_prompt: &str) -> Result<Option<ChatCompletionRequestMessage>> {
        if system_prompt.is_empty() {
            return Ok(None);
        }
        let message = ChatCompletionRequestSystemMessageArgs::default()
            .content(system_prompt)
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(Some(ChatCompletionRequestMessage::System(message)))
    }

    fn build_user_text_message(prompt: &str) -> Result<ChatCompletionRequestMessage> {
        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(ChatCompletionRequestUserMessageContent::Text(
                prompt.to_string(),
            ))
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(ChatCompletionRequestMessage::User(message))
    }

    fn build_user_image_message(
        prompt: &str,
        images: Vec<ImageInput>,
        detail: &str,
    ) -> Result<ChatCompletionRequestMessage> {
        let mut parts = Vec::new();
        parts.push(ChatCompletionRequestUserMessageContentPart::Text(
            ChatCompletionRequestMessageContentPartText {
                text: prompt.to_string(),
            },
        ));

        let detail = Self::image_detail(detail);
        for image in images {
            let image_url = match image.data {
                ImageData::Url(url) => url,
                ImageData::Bytes(bytes) => {
                    let encoded = general_purpose::STANDARD.encode(bytes);
                    format!("data:image/png;base64,{}", encoded)
                }
            };
            let image_part = ChatCompletionRequestMessageContentPartImage {
                image_url: ImageUrl {
                    url: image_url,
                    detail: Some(detail.clone()),
                },
            };
            parts.push(ChatCompletionRequestUserMessageContentPart::ImageUrl(
                image_part,
            ));
        }

        let message = ChatCompletionRequestUserMessageArgs::default()
            .content(ChatCompletionRequestUserMessageContent::Array(parts))
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(ChatCompletionRequestMessage::User(message))
    }

    fn image_detail(detail: &str) -> ImageDetail {
        match detail.to_lowercase().as_str() {
            "low" => ImageDetail::Low,
            "high" => ImageDetail::High,
            _ => ImageDetail::Auto,
        }
    }

    fn convert_tools(tools: Vec<Value>) -> Vec<ChatCompletionTools> {
        tools
            .into_iter()
            .filter_map(|tool| {
                let tool_type = tool
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("function");
                if tool_type != "function" {
                    return None;
                }
                let function_obj = tool.get("function").cloned().unwrap_or(tool);
                let name = function_obj.get("name")?.as_str()?.trim().to_string();
                if !Self::is_openai_function_name(&name) {
                    warn!(tool_name = %name, "Skipping invalid OpenAI function tool name");
                    return None;
                }
                let description = function_obj
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                let parameters = function_obj
                    .get("parameters")
                    .cloned()
                    .filter(|value| value.is_object())
                    .or_else(|| {
                        Some(serde_json::json!({
                            "type": "object",
                            "properties": {},
                            "additionalProperties": true
                        }))
                    });
                let function = FunctionObject {
                    name,
                    description,
                    parameters,
                    strict: Some(false),
                };
                Some(ChatCompletionTools::Function(ChatCompletionTool {
                    function,
                }))
            })
            .collect()
    }

    fn extract_text_from_response(
        response: &async_openai::types::chat::CreateChatCompletionResponse,
    ) -> Result<String> {
        let message = response
            .choices
            .first()
            .ok_or_else(|| ButterflyBotError::Runtime("No choices returned".to_string()))?
            .message
            .content
            .clone()
            .unwrap_or_default();
        Ok(message)
    }

    fn extract_tool_calls_from_response(
        response: &async_openai::types::chat::CreateChatCompletionResponse,
    ) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        let Some(choice) = response.choices.first() else {
            return calls;
        };
        let message = &choice.message;
        if let Some(tool_calls) = &message.tool_calls {
            for call in tool_calls {
                match call {
                    ChatCompletionMessageToolCalls::Function(function_call) => {
                        let name = function_call.function.name.clone();
                        let args = function_call.function.arguments.clone();
                        let arguments = serde_json::from_str(&args).unwrap_or(Value::String(args));
                        calls.push(ToolCall { name, arguments });
                    }
                    ChatCompletionMessageToolCalls::Custom(custom_call) => {
                        let name = custom_call.custom_tool.name.clone();
                        let args = custom_call.custom_tool.input.clone();
                        let arguments = serde_json::from_str(&args).unwrap_or(Value::String(args));
                        calls.push(ToolCall { name, arguments });
                    }
                }
            }
        }

        if calls.is_empty() {
            #[allow(deprecated)]
            if let Some(FunctionCall { name, arguments }) = &message.function_call {
                let parsed =
                    serde_json::from_str(arguments).unwrap_or(Value::String(arguments.clone()));
                calls.push(ToolCall {
                    name: name.clone(),
                    arguments: parsed,
                });
            }
        }

        calls
    }

    fn voice_from_str(voice: &str) -> Voice {
        match voice.to_lowercase().as_str() {
            "alloy" => Voice::Alloy,
            "ash" => Voice::Ash,
            "ballad" => Voice::Ballad,
            "coral" => Voice::Coral,
            "echo" => Voice::Echo,
            "fable" => Voice::Fable,
            "onyx" => Voice::Onyx,
            "nova" => Voice::Nova,
            "sage" => Voice::Sage,
            "shimmer" => Voice::Shimmer,
            "verse" => Voice::Verse,
            other => Voice::Other(other.to_string()),
        }
    }

    fn speech_format_from_str(format: &str) -> SpeechResponseFormat {
        match format.to_lowercase().as_str() {
            "opus" => SpeechResponseFormat::Opus,
            "aac" => SpeechResponseFormat::Aac,
            "flac" => SpeechResponseFormat::Flac,
            "wav" => SpeechResponseFormat::Wav,
            "pcm" | "pcm16" => SpeechResponseFormat::Pcm,
            _ => SpeechResponseFormat::Mp3,
        }
    }

    fn prefer_structured_tool_json_mode(&self) -> bool {
        false
    }

    fn extract_tool_names(tools: &[Value]) -> Vec<String> {
        let mut names = Vec::new();
        for tool in tools {
            let name = tool
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| tool.get("name"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
            if let Some(name) = name {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    fn structured_tool_loop_schema(tools: &[Value]) -> Value {
        let names = Self::extract_tool_names(tools);
        let name_schema = if names.is_empty() {
            serde_json::json!({"type": "string"})
        } else {
            serde_json::json!({"type": "string", "enum": names})
        };

        serde_json::json!({
            "title": "tool_loop_response",
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "tool_calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": name_schema,
                            "arguments": {
                                "type": "object",
                                "additionalProperties": true
                            }
                        },
                        "required": ["name", "arguments"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["text", "tool_calls"],
            "additionalProperties": false
        })
    }

    fn llm_response_from_structured(value: &Value) -> Option<LlmResponse> {
        let text = value
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let tool_calls = value
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let name = call.get("name")?.as_str()?.to_string();
                        let arguments = call
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({}));
                        Some(ToolCall { name, arguments })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(LlmResponse { text, tool_calls })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn generate_text(
        &self,
        prompt: &str,
        system_prompt: &str,
        tools: Option<Vec<Value>>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(system) = Self::build_system_message(system_prompt)? {
            messages.push(system);
        }
        messages.push(Self::build_user_text_message(prompt)?);

        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(self.model.clone());
        builder.messages(messages);

        if let Some(tools) = tools {
            let tools = Self::convert_tools(tools);
            if !tools.is_empty() {
                builder.tools(tools);
            }
        }

        let request = builder
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self.chat_create_with_fallback(request).await?;
        match response {
            ChatCreateResult::Parsed(parsed) => Self::extract_text_from_response(&parsed),
            ChatCreateResult::Raw(raw) => Self::extract_text_from_value(&raw)
                .ok_or_else(|| ButterflyBotError::Runtime("Empty chat response".to_string())),
        }
    }

    async fn embed(&self, inputs: Vec<String>, model: Option<&str>) -> Result<Vec<Vec<f32>>> {
        let model = model.unwrap_or(&self.model).to_string();
        let request = CreateEmbeddingRequestArgs::default()
            .model(model)
            .input(EmbeddingInput::StringArray(inputs))
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        let response = self
            .client
            .embeddings()
            .create(request)
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        let mut data = response.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| item.embedding).collect())
    }
    async fn generate_with_tools(
        &self,
        prompt: &str,
        system_prompt: &str,
        tools: Vec<Value>,
    ) -> Result<LlmResponse> {
        if self.prefer_structured_tool_json_mode() && !tools.is_empty() {
            let schema = Self::structured_tool_loop_schema(&tools);
            let structured_prompt = format!(
                "{}\n\nRespond ONLY as strict JSON matching the schema. Do not include markdown or extra text.",
                prompt
            );
            if let Ok(value) = self
                .parse_structured_output(&structured_prompt, system_prompt, schema, None)
                .await
            {
                if let Some(response) = Self::llm_response_from_structured(&value) {
                    return Ok(response);
                }
            }
        }

        let mut messages = Vec::new();
        if let Some(system) = Self::build_system_message(system_prompt)? {
            messages.push(system);
        }
        messages.push(Self::build_user_text_message(prompt)?);

        let tools = Self::convert_tools(tools);
        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(self.model.clone());
        builder.messages(messages);
        if !tools.is_empty() {
            builder.tools(tools);
        }

        let request = builder
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self.chat_create_with_fallback(request).await?;
        let (text, tool_calls) = match response {
            ChatCreateResult::Parsed(parsed) => {
                let text = Self::extract_text_from_response(&parsed).unwrap_or_default();
                let tool_calls = Self::extract_tool_calls_from_response(&parsed);
                (text, tool_calls)
            }
            ChatCreateResult::Raw(raw) => {
                let text = Self::extract_text_from_value(&raw).unwrap_or_default();
                let tool_calls = Self::extract_tool_calls_from_value(&raw);
                (text, tool_calls)
            }
        };

        Ok(LlmResponse { text, tool_calls })
    }

    fn chat_stream(
        &self,
        messages: Vec<Value>,
        tools: Option<Vec<Value>>,
    ) -> BoxStream<'static, Result<ChatEvent>> {
        let provider = self.clone();

        Box::pin(try_stream! {
            let mut request_messages = Vec::new();
            for message in messages {
                let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                let content = message.get("content").and_then(|v| v.as_str()).unwrap_or("");
                match role {
                    "system" => {
                        if let Some(msg) = OpenAiProvider::build_system_message(content)? {
                            request_messages.push(msg);
                        }
                    }
                    "user" => {
                        request_messages.push(OpenAiProvider::build_user_text_message(content)?);
                    }
                    _ => {
                        request_messages.push(OpenAiProvider::build_user_text_message(content)?);
                    }
                }
            }

            let mut builder = CreateChatCompletionRequestArgs::default();
            builder.model(provider.model.clone());
            builder.messages(request_messages);

            if let Some(tools) = tools {
                let tools = OpenAiProvider::convert_tools(tools);
                if !tools.is_empty() {
                    builder.tools(tools);
                }
            }

            let request = builder
                .build()
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

            let response = provider.chat_create_with_fallback(request).await?;
            let content = match response {
                ChatCreateResult::Parsed(parsed) => {
                    OpenAiProvider::extract_text_from_response(&parsed).unwrap_or_default()
                }
                ChatCreateResult::Raw(raw) => {
                    OpenAiProvider::extract_text_from_value(&raw).unwrap_or_default()
                }
            };

            if !content.is_empty() {
                yield ChatEvent {
                    event_type: "content".to_string(),
                    delta: Some(content),
                    name: None,
                    arguments_delta: None,
                    finish_reason: None,
                    error: None,
                };
            }

            yield ChatEvent {
                event_type: "message_end".to_string(),
                delta: None,
                name: None,
                arguments_delta: None,
                finish_reason: Some("stop".to_string()),
                error: None,
            };
        })
    }

    async fn parse_structured_output(
        &self,
        prompt: &str,
        system_prompt: &str,
        json_schema: Value,
        tools: Option<Vec<Value>>,
    ) -> Result<Value> {
        let mut messages = Vec::new();
        if let Some(system) = Self::build_system_message(system_prompt)? {
            messages.push(system);
        }
        messages.push(Self::build_user_text_message(prompt)?);

        let name = json_schema
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("structured_output")
            .to_string();
        let response_format = ResponseFormat::JsonSchema {
            json_schema: ResponseFormatJsonSchema {
                name,
                description: None,
                schema: Some(json_schema),
                strict: Some(true),
            },
        };

        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(self.model.clone());
        builder.messages(messages);
        builder.response_format(response_format);

        if let Some(tools) = tools {
            let tools = Self::convert_tools(tools);
            if !tools.is_empty() {
                builder.tools(tools);
            }
        }

        let request = builder
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self.chat_create_with_fallback(request).await?;
        let content = match response {
            ChatCreateResult::Parsed(parsed) => Self::extract_text_from_response(&parsed)?,
            ChatCreateResult::Raw(raw) => Self::extract_text_from_value(&raw)
                .ok_or_else(|| ButterflyBotError::Runtime("Empty chat response".to_string()))?,
        };
        let parsed = serde_json::from_str(&content)
            .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
        Ok(parsed)
    }

    async fn tts(&self, text: &str, voice: &str, response_format: &str) -> Result<Vec<u8>> {
        let request = CreateSpeechRequestArgs::default()
            .model(SpeechModel::Tts1)
            .input(text)
            .voice(Self::voice_from_str(voice))
            .response_format(Self::speech_format_from_str(response_format))
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self
            .client
            .audio()
            .speech()
            .create(request)
            .await
            .map_err(|e| ButterflyBotError::Http(e.to_string()))?;

        Ok(response.bytes.to_vec())
    }

    async fn transcribe_audio(&self, audio_bytes: Vec<u8>, input_format: &str) -> Result<String> {
        let file = AudioInput {
            source: InputSource::VecU8 {
                filename: format!("audio.{}", input_format),
                vec: audio_bytes,
            },
        };

        let request = CreateTranscriptionRequestArgs::default()
            .file(file)
            .model("gpt-4o-mini-transcribe")
            .response_format(AudioResponseFormat::Json)
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self
            .client
            .audio()
            .transcription()
            .create(request)
            .await
            .map_err(|e| ButterflyBotError::Http(e.to_string()))?;

        Ok(response.text)
    }

    async fn generate_text_with_images(
        &self,
        prompt: &str,
        images: Vec<ImageInput>,
        system_prompt: &str,
        detail: &str,
        tools: Option<Vec<Value>>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(system) = Self::build_system_message(system_prompt)? {
            messages.push(system);
        }
        messages.push(Self::build_user_image_message(prompt, images, detail)?);

        let mut builder = CreateChatCompletionRequestArgs::default();
        builder.model(self.model.clone());
        builder.messages(messages);

        if let Some(tools) = tools {
            let tools = Self::convert_tools(tools);
            if !tools.is_empty() {
                builder.tools(tools);
            }
        }

        let request = builder
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let response = self.chat_create_with_fallback(request).await?;
        match response {
            ChatCreateResult::Parsed(parsed) => Self::extract_text_from_response(&parsed),
            ChatCreateResult::Raw(raw) => Self::extract_text_from_value(&raw)
                .ok_or_else(|| ButterflyBotError::Runtime("Empty chat response".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OpenAiProvider;
    use async_openai::types::chat::ChatCompletionTools;
    use serde_json::json;

    #[test]
    fn prefers_structured_mode_is_disabled() {
        let provider = OpenAiProvider::new(
            "key".to_string(),
            Some("gpt-5.2".to_string()),
            Some("https://api.openai.com/v1".to_string()),
        );
        assert!(!provider.prefer_structured_tool_json_mode());
    }

    #[test]
    fn converts_structured_tool_response() {
        let value = json!({
            "text": "ok",
            "tool_calls": [
                {"name": "solana", "arguments": {"action": "balance"}}
            ]
        });
        let response = OpenAiProvider::llm_response_from_structured(&value).unwrap();
        assert_eq!(response.text, "ok");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "solana");
        assert_eq!(response.tool_calls[0].arguments["action"], json!("balance"));
    }

    #[test]
    fn convert_tools_emits_boolean_strict_and_object_parameters() {
        let tools = OpenAiProvider::convert_tools(vec![json!({
            "type": "function",
            "name": "todo",
            "description": "todo",
            "parameters": {"type": "object", "properties": {}}
        })]);

        assert_eq!(tools.len(), 1);
        match &tools[0] {
            ChatCompletionTools::Function(tool) => {
                assert_eq!(tool.function.name, "todo");
                assert_eq!(tool.function.strict, Some(false));
                assert!(tool
                    .function
                    .parameters
                    .as_ref()
                    .is_some_and(|p| p.is_object()));
            }
            _ => panic!("expected function tool"),
        }
    }

    #[test]
    fn convert_tools_fills_missing_parameters_with_object_schema() {
        let tools = OpenAiProvider::convert_tools(vec![json!({
            "type": "function",
            "name": "tasks"
        })]);

        assert_eq!(tools.len(), 1);
        match &tools[0] {
            ChatCompletionTools::Function(tool) => {
                assert_eq!(tool.function.name, "tasks");
                assert_eq!(tool.function.strict, Some(false));
                let params = tool
                    .function
                    .parameters
                    .as_ref()
                    .expect("parameters required");
                assert_eq!(params.get("type").and_then(|v| v.as_str()), Some("object"));
            }
            _ => panic!("expected function tool"),
        }
    }

    #[test]
    fn convert_tools_skips_invalid_openai_function_names() {
        let tools = OpenAiProvider::convert_tools(vec![
            json!({"type":"function","name":"solana.getBalance","parameters":{}}),
            json!({"type":"function","name":"solana_get_balance","parameters":{}}),
        ]);

        assert_eq!(tools.len(), 1);
        match &tools[0] {
            ChatCompletionTools::Function(tool) => {
                assert_eq!(tool.function.name, "solana_get_balance");
            }
            _ => panic!("expected function tool"),
        }
    }
}
