use serde_json::{json, Value};

const SYSTEM_PROMPT: &str = "You are a prompt structurer. Take the raw speech transcript and restructure it into a clean prompt ready for an LLM. Create relevant sections (Context, Goal, Expected Output, etc) based on content. Do NOT use same sections every time. Do NOT add info not mentioned. Output ONLY the structured prompt with ## markdown headers. Be concise, preserve all details.";

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
        "openai" => structure_with_openai(transcript, model, api_key).await,
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

async fn structure_with_openai(text: &str, model: &str, api_key: &str) -> Result<String, String> {
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
        .post("https://api.openai.com/v1/chat/completions")
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
