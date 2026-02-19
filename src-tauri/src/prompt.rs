//! Cloud LLM prompt structuring.
//!
//! Takes raw transcribed speech and sends it to a cloud LLM (Anthropic, OpenAI,
//! or OpenAI-compatible like OpenRouter) to restructure it into a well-organized
//! first-person prompt. Entry point: [`structure_prompt`].
//! Caller should fallback to raw text on any error — never lose the transcription.

use serde_json::{json, Value};

const SYSTEM_PROMPT: &str = "You are a prompt structurer. Take my raw speech transcript and restructure it into a clean, first-person prompt ready to paste into an LLM. Write it as ME talking to the AI (use 'I want', 'I need', 'my project', etc — not 'the speaker' or 'the user'). Create relevant sections based on what I described (e.g. Context, Goal, Expected Output, Constraints, Tech Stack — adapt to the content, don't use the same sections every time). Do NOT add information I didn't mention. Output ONLY the structured prompt with ## markdown headers. Be concise but preserve all important details.";

pub async fn structure_prompt(
    text: &str,
    provider: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let transcript = text.trim();
    if transcript.is_empty() {
        return Err("cannot structure empty text".to_string());
    }

    let provider = provider.trim().to_lowercase();
    match provider.as_str() {
        "anthropic" => structure_with_anthropic(transcript, model, api_key).await,
        "openai" => {
            structure_with_openai_compat(
                transcript,
                model,
                api_key,
                "https://api.openai.com/v1/chat/completions",
            )
            .await
        }
        "openrouter" => {
            structure_with_openai_compat(
                transcript,
                model,
                api_key,
                "https://openrouter.ai/api/v1/chat/completions",
            )
            .await
        }
        _ => Err(format!("unsupported prompt provider: {provider}")),
    }
}

async fn structure_with_anthropic(
    text: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": 1024,
        "system": SYSTEM_PROMPT,
        "messages": [
            {
                "role": "user",
                "content": text
            }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("anthropic request failed: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read anthropic response: {e}"))?;

    if !status.is_success() {
        let detail = extract_error_message(&body).unwrap_or(body);
        return Err(format!("anthropic request failed ({status}): {detail}"));
    }

    extract_anthropic_text(&body)
}

async fn structure_with_openai_compat(
    text: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": SYSTEM_PROMPT
            },
            {
                "role": "user",
                "content": text
            }
        ]
    });

    let client = reqwest::Client::new();
    let response = client
        .post(base_url)
        .header("authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("openai request failed: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read openai response: {e}"))?;

    if !status.is_success() {
        let detail = extract_error_message(&body).unwrap_or(body);
        return Err(format!("openai request failed ({status}): {detail}"));
    }

    extract_openai_text(&body)
}

fn extract_anthropic_text(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid anthropic response JSON: {e}"))?;

    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "anthropic response missing content array".to_string())?;

    let mut parts: Vec<&str> = Vec::new();
    for item in content {
        if item.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                parts.push(text.trim());
            }
        }
    }

    if parts.is_empty() {
        return Err("anthropic response had no text blocks".to_string());
    }

    Ok(parts.join("\n\n"))
}

fn extract_openai_text(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid openai response JSON: {e}"))?;

    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .ok_or_else(|| "openai response missing choices[0].message.content".to_string())?;

    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Err("openai response content was empty".to_string())
            } else {
                Ok(trimmed.to_string())
            }
        }
        Value::Array(items) => {
            let mut parts: Vec<&str> = Vec::new();
            for item in items {
                let text = item.get("text").and_then(Value::as_str);
                if let Some(text) = text {
                    if !text.trim().is_empty() {
                        parts.push(text.trim());
                    }
                }
            }
            if parts.is_empty() {
                Err("openai response content array had no text blocks".to_string())
            } else {
                Ok(parts.join("\n\n"))
            }
        }
        _ => Err("openai response content format is unsupported".to_string()),
    }
}

fn extract_error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    if let Some(message) = error.as_str() {
        return Some(message.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_single_text_block() {
        let body = r###"{"content":[{"type":"text","text":"## Context\nUser wants X"}]}"###;
        let result = extract_anthropic_text(body).unwrap();
        assert_eq!(result, "## Context\nUser wants X");
    }

    #[test]
    fn anthropic_multiple_text_blocks() {
        let body = r#"{"content":[{"type":"text","text":"Part 1"},{"type":"text","text":"Part 2"}]}"#;
        let result = extract_anthropic_text(body).unwrap();
        assert_eq!(result, "Part 1\n\nPart 2");
    }

    #[test]
    fn anthropic_empty_content_array() {
        let body = r#"{"content":[]}"#;
        assert!(extract_anthropic_text(body).is_err());
    }

    #[test]
    fn anthropic_missing_content_key() {
        let body = r#"{"id":"msg_123"}"#;
        assert!(extract_anthropic_text(body).is_err());
    }

    #[test]
    fn openai_standard_response() {
        let body = r###"{"choices":[{"message":{"content":"## Goal\nBuild a thing"}}]}"###;
        let result = extract_openai_text(body).unwrap();
        assert_eq!(result, "## Goal\nBuild a thing");
    }

    #[test]
    fn openai_empty_choices() {
        let body = r#"{"choices":[]}"#;
        assert!(extract_openai_text(body).is_err());
    }

    #[test]
    fn openai_blank_content() {
        let body = r#"{"choices":[{"message":{"content":"  "}}]}"#;
        assert!(extract_openai_text(body).is_err());
    }

    #[test]
    fn error_message_object_style() {
        let body = r#"{"error":{"message":"rate limit exceeded","type":"rate_limit"}}"#;
        assert_eq!(extract_error_message(body), Some("rate limit exceeded".to_string()));
    }

    #[test]
    fn error_message_string_style() {
        let body = r#"{"error":"something went wrong"}"#;
        assert_eq!(extract_error_message(body), Some("something went wrong".to_string()));
    }

    #[test]
    fn error_message_no_error_key() {
        let body = r#"{"content":[]}"#;
        assert_eq!(extract_error_message(body), None);
    }

    #[test]
    fn error_message_invalid_json() {
        assert_eq!(extract_error_message("not json"), None);
    }

    #[tokio::test]
    async fn empty_text_returns_error() {
        let result = structure_prompt("  ", "anthropic", "model", "key").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[tokio::test]
    async fn unknown_provider_returns_error() {
        let result = structure_prompt("hello", "gemini", "model", "key").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported"));
    }
}
