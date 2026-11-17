use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::config::Config;
use crate::AppState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
        pub text: String,
        pub scene_id: Option<String>,
        pub target_lang: Option<String>,
        pub source_lang: Option<String>,
        pub target_window: String,
        pub request_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkPayload {
    request_id: String,
    delta: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DonePayload {
    request_id: String,
    full_text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload {
    request_id: String,
    message: String,
}

fn lang_label(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        "自动检测到的语言".to_string()
    } else {
        trimmed.to_string()
    }
}

fn render_prompt(template: &str, source: &str, target: &str, text: &str) -> String {
    template
        .replace("{source}", source)
        .replace("{target}", target)
        .replace("{text}", text)
}

fn pick_prompt(cfg: &Config, scene_id: Option<&str>) -> String {
    let wanted = scene_id.unwrap_or(&cfg.active_scene_id);
    let raw = cfg
        .scenes
        .iter()
        .find(|s| s.id == wanted)
        .or_else(|| cfg.scenes.iter().find(|s| s.id == cfg.active_scene_id))
        .or_else(|| cfg.scenes.first())
        .map(|s| s.prompt.clone())
        .unwrap_or_default();

            if raw.trim().is_empty() {
        crate::config::DEFAULT_GENERAL_PROMPT.to_string()
    } else {
        raw
    }
}

pub fn emit_error(app: &AppHandle, window: &str, request_id: &str, message: &str) {
    let _ = app.emit_to(
        window,
        "translate-error",
        ErrorPayload {
            request_id: request_id.to_string(),
            message: message.to_string(),
        },
    );
}

pub async fn run_translate(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    req: TranslateRequest,
) -> Result<(), String> {
    let cfg = state.config(&app);
    let prompt_template = pick_prompt(&cfg, req.scene_id.as_deref());

    let target_window = req.target_window.clone();
    let request_id = req.request_id.clone();

    if cfg.api.api_key.trim().is_empty() {
                let _ = app.emit_to(&target_window, "need-api-key", ());
        return Ok(());
    }

    let text = req.text.trim().to_string();
    if text.is_empty() {
        emit_error(&app, &target_window, &request_id, "没有可翻译的文本。");
        return Ok(());
    }

    let source = lang_label(req.source_lang.as_deref().unwrap_or(&cfg.languages.source));
    let target = lang_label(req.target_lang.as_deref().unwrap_or(&cfg.languages.target));
    let system_prompt = render_prompt(&prompt_template, &source, &target, &text);

    let url = format!(
        "{}/chat/completions",
        cfg.api.base_url.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "model": cfg.api.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": text }
        ],
        "stream": cfg.stream,
        "temperature": cfg.api.temperature
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = match client
        .post(&url)
        .bearer_auth(cfg.api.api_key.trim())
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            emit_error(
                &app,
                &target_window,
                &request_id,
                &format!("请求 DeepSeek 失败：{e}"),
            );
            return Ok(());
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        let brief = extract_api_error(&detail);
        emit_error(
            &app,
            &target_window,
            &request_id,
            &format!("DeepSeek 返回 {status}：{brief}"),
        );
        return Ok(());
    }

        if !cfg.stream {
        let payload: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                emit_error(&app, &target_window, &request_id, &format!("解析响应失败：{e}"));
                return Ok(());
            }
        };
        let full = payload["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let _ = app.emit_to(
            &target_window,
            "translate-chunk",
            ChunkPayload {
                request_id: request_id.clone(),
                delta: full.clone(),
            },
        );
        let _ = app.emit_to(
            &target_window,
            "translate-done",
            DonePayload {
                request_id,
                full_text: full,
            },
        );
        return Ok(());
    }

        let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => {
                emit_error(&app, &target_window, &request_id, &format!("流式读取中断：{e}"));
                break;
            }
        };
        buf.extend_from_slice(&bytes);

        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(['\r', '\n']);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(delta) = value["choices"][0]["delta"]["content"].as_str() {
                if !delta.is_empty() {
                    full.push_str(delta);
                    let _ = app.emit_to(
                        &target_window,
                        "translate-chunk",
                        ChunkPayload {
                            request_id: request_id.clone(),
                            delta: delta.to_string(),
                        },
                    );
                }
            }
        }
    }

    let _ = app.emit_to(
        &target_window,
        "translate-done",
        DonePayload {
            request_id,
            full_text: full,
        },
    );

    Ok(())
}

fn extract_api_error(raw: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(msg) = v["error"]["message"].as_str() {
            return msg.to_string();
        }
        if let Some(msg) = v["message"].as_str() {
            return msg.to_string();
        }
    }
    let s = raw.trim();
    if s.is_empty() {
        "无详细信息".to_string()
    } else {
        s.chars().take(300).collect()
    }
}

#[tauri::command]
pub async fn translate(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    req: TranslateRequest,
) -> Result<(), String> {
    run_translate(app, state, req).await
}

#[tauri::command]
pub async fn fetch_models(base_url: String, api_key: String) -> Result<Vec<String>, String> {
    let key = api_key.trim().to_string();
    if key.is_empty() {
        return Err("请先填写 API Key".into());
    }

    let base = if base_url.trim().is_empty() {
        "https://api.deepseek.com".to_string()
    } else {
        base_url.trim().trim_end_matches('/').to_string()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(format!("{base}/models"))
        .bearer_auth(&key)
        .send()
        .await
        .map_err(|e| format!("网络错误：{e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}：{}", extract_api_error(&detail)));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败：{e}"))?;

    let mut models: Vec<String> = body
        .get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m.get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    if models.is_empty() {
        return Err("接口没有返回任何模型".into());
    }

    models.sort();
    Ok(models)
}

#[tauri::command]
pub async fn test_api(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config(&app).api;

    if cfg.api_key.trim().is_empty() {
        return Err("API Key 为空".into());
    }

    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 3,
        "stream": false
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&url)
        .bearer_auth(cfg.api_key.trim())
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("网络错误：{e}"))?;

    if resp.status().is_success() {
        let _ = app;
        Ok(format!("连接成功（HTTP {}）", resp.status()))
    } else {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {status}：{}", extract_api_error(&detail)))
    }
}
