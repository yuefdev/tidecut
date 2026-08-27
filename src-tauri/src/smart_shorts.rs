use std::{collections::HashSet, fs, io::Read, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::render::RenderErrorReport;

const FIREWORKS_ENDPOINT: &str = "https://api.fireworks.ai/inference/v1/chat/completions";
const FIREWORKS_MODEL: &str = "accounts/fireworks/models/glm-5p2";
const MAX_CANDIDATES: usize = 12;
const MAX_TRANSCRIPT_CHARS: usize = 4_000;
const MAX_CANDIDATE_ID_CHARS: usize = 160;
const MAX_EVIDENCE_ITEMS: usize = 12;
const MAX_EVIDENCE_ITEM_CHARS: usize = 480;
const MAX_TOTAL_PROMPT_CHARS: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;
const MAX_SESSION_API_KEY_CHARS: usize = 1_024;
const MAX_PROJECT_ENV_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireworksSmartShortsStatus {
    pub configured: bool,
    pub model: &'static str,
    pub privacy_mode: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankSmartShortsRequest {
    #[serde(default)]
    pub api_key: Option<String>,
    pub candidates: Vec<SmartShortsCandidatePrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartShortsCandidatePrompt {
    pub candidate_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub local_score: f64,
    pub transcript: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SmartShortsGlmRanking {
    pub candidate_id: String,
    pub score: f64,
    pub reason: String,
    pub focus_target: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankSmartShortsResponse {
    pub model: &'static str,
    pub rankings: Vec<SmartShortsGlmRanking>,
}

#[derive(Debug, Deserialize)]
struct FireworksEnvelope {
    choices: Vec<FireworksChoice>,
}

#[derive(Debug, Deserialize)]
struct FireworksChoice {
    message: FireworksMessage,
}

#[derive(Debug, Deserialize)]
struct FireworksMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct FireworksRankingPayload {
    rankings: Vec<SmartShortsGlmRanking>,
}

#[tauri::command]
pub fn check_fireworks_smart_shorts() -> FireworksSmartShortsStatus {
    FireworksSmartShortsStatus {
        configured: environment_api_key().is_some(),
        model: FIREWORKS_MODEL,
        privacy_mode: "transcript-only-store-false",
    }
}

#[tauri::command]
pub async fn rank_smart_short_candidates(
    request: RankSmartShortsRequest,
) -> Result<RankSmartShortsResponse, RenderErrorReport> {
    validate_request(&request)?;
    let api_key = request
        .api_key
        .as_deref()
        .and_then(non_empty_secret)
        .or_else(environment_api_key)
        .ok_or_else(|| {
            smart_shorts_error(
                "fireworks_not_configured",
                "Fireworks anahtarı ayarlı değil; yerel puanlama kullanılacak.",
                "FIREWORKS_API_KEY was not set and the request did not include a session key",
                false,
            )
        })?;
    tauri::async_runtime::spawn_blocking(move || rank_with_fireworks(&api_key, &request))
        .await
        .map_err(|error| {
            smart_shorts_error(
                "fireworks_worker_failed",
                "GLM aday sıralaması tamamlanamadı; yerel sonuçlar korunacak.",
                error.to_string(),
                true,
            )
        })?
}

fn rank_with_fireworks(
    api_key: &str,
    request: &RankSmartShortsRequest,
) -> Result<RankSmartShortsResponse, RenderErrorReport> {
    let body = build_request_body(request);
    let encoded = serde_json::to_string(&body).map_err(|error| {
        smart_shorts_error(
            "fireworks_request_encode_failed",
            "GLM isteği hazırlanamadı; yerel sonuçlar korunacak.",
            error.to_string(),
            false,
        )
    })?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(50)))
        .build()
        .into();
    let mut response = agent
        .post(FIREWORKS_ENDPOINT)
        .header("Authorization", &format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .send(encoded.as_bytes())
        .map_err(|error| {
            smart_shorts_error(
                "fireworks_request_failed",
                "GLM aday sıralaması ulaşılamadı; yerel sonuçlar korunacak.",
                error.to_string(),
                true,
            )
        })?;
    let mut response_text = String::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_string(&mut response_text)
        .map_err(|error| {
            smart_shorts_error(
                "fireworks_response_read_failed",
                "GLM yanıtı okunamadı; yerel sonuçlar korunacak.",
                error.to_string(),
                true,
            )
        })?;
    if response_text.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(smart_shorts_error(
            "fireworks_response_too_large",
            "GLM beklenenden büyük bir yanıt döndürdü; yerel sonuçlar korunacak.",
            format!("Response exceeded {MAX_RESPONSE_BYTES} bytes"),
            false,
        ));
    }
    parse_fireworks_response(&response_text, &request.candidates)
}

fn build_request_body(request: &RankSmartShortsRequest) -> serde_json::Value {
    let candidate_json = serde_json::to_string(&request.candidates).unwrap_or_else(|_| "[]".into());
    serde_json::json!({
        "model": FIREWORKS_MODEL,
        "store": false,
        "temperature": 0.15,
        "max_tokens": 1800,
        "messages": [
            {
                "role": "system",
                "content": "Sen profesyonel bir kısa video editörüsün. Yalnız verilen adayları değerlendir. Komiklik, merak uyandıran açılış, net bağlam, tempo ve paylaşılabilir payoff ölçütlerini kullan. Kanıt yoksa kahkaha veya viral başarı uydurma. candidateId değerlerini aynen koru."
            },
            {
                "role": "user",
                "content": format!("Bu zaman damgalı adayları Türkçe kısa video için sırala. Ham video gönderilmedi; yalnız transkript ve yerel sinyaller var:\n{candidate_json}")
            }
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "smart_shorts_ranking",
                "strict": true,
                "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "rankings": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "candidateId": { "type": "string" },
                                    "score": { "type": "number", "minimum": 0, "maximum": 100 },
                                    "reason": { "type": "string" },
                                    "focusTarget": { "type": "string" }
                                },
                                "required": ["candidateId", "score", "reason", "focusTarget"]
                            }
                        }
                    },
                    "required": ["rankings"]
                }
            }
        }
    })
}

fn parse_fireworks_response(
    response_text: &str,
    candidates: &[SmartShortsCandidatePrompt],
) -> Result<RankSmartShortsResponse, RenderErrorReport> {
    let envelope: FireworksEnvelope = serde_json::from_str(response_text).map_err(|error| {
        smart_shorts_error(
            "fireworks_response_invalid",
            "GLM geçerli bir sonuç döndürmedi; yerel sonuçlar korunacak.",
            error.to_string(),
            true,
        )
    })?;
    let content = envelope
        .choices
        .first()
        .map(|choice| choice.message.content.trim())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| {
            smart_shorts_error(
                "fireworks_response_empty",
                "GLM boş sonuç döndürdü; yerel sonuçlar korunacak.",
                "choices[0].message.content was missing",
                true,
            )
        })?;
    let payload: FireworksRankingPayload = serde_json::from_str(content).map_err(|error| {
        smart_shorts_error(
            "fireworks_schema_invalid",
            "GLM sonuç şeması doğrulanamadı; yerel sonuçlar korunacak.",
            error.to_string(),
            true,
        )
    })?;
    let allowed = candidates
        .iter()
        .map(|candidate| candidate.candidate_id.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let rankings = payload
        .rankings
        .into_iter()
        .filter_map(|mut ranking| {
            if !allowed.contains(ranking.candidate_id.as_str())
                || !seen.insert(ranking.candidate_id.clone())
            {
                return None;
            }
            ranking.score = ranking.score.clamp(0.0, 100.0);
            ranking.reason = ranking.reason.trim().chars().take(320).collect();
            ranking.focus_target = ranking.focus_target.trim().chars().take(120).collect();
            Some(ranking)
        })
        .collect::<Vec<_>>();
    if rankings.is_empty() {
        return Err(smart_shorts_error(
            "fireworks_rankings_empty",
            "GLM güvenilir bir aday eşleştiremedi; yerel sonuçlar korunacak.",
            "No ranking referenced a supplied candidateId",
            true,
        ));
    }
    Ok(RankSmartShortsResponse {
        model: FIREWORKS_MODEL,
        rankings,
    })
}

fn validate_request(request: &RankSmartShortsRequest) -> Result<(), RenderErrorReport> {
    if request.candidates.is_empty() || request.candidates.len() > MAX_CANDIDATES {
        return Err(smart_shorts_error(
            "fireworks_candidates_invalid",
            "GLM için 1–12 kısa video adayı gerekir.",
            format!("candidate count was {}", request.candidates.len()),
            false,
        ));
    }
    if request
        .api_key
        .as_deref()
        .is_some_and(|value| value.chars().count() > MAX_SESSION_API_KEY_CHARS)
    {
        return Err(smart_shorts_error(
            "fireworks_api_key_invalid",
            "Fireworks oturum anahtarı geçersiz.",
            "Session API key exceeded the maximum length",
            false,
        ));
    }
    let mut ids = HashSet::new();
    let mut total_prompt_chars = 0_usize;
    for candidate in &request.candidates {
        let id_chars = candidate.candidate_id.chars().count();
        let transcript_chars = candidate.transcript.chars().count();
        let evidence_chars = candidate
            .evidence
            .iter()
            .map(|item| item.chars().count())
            .sum::<usize>();
        total_prompt_chars = total_prompt_chars
            .saturating_add(id_chars)
            .saturating_add(transcript_chars)
            .saturating_add(evidence_chars);
        if candidate.candidate_id.trim().is_empty()
            || id_chars > MAX_CANDIDATE_ID_CHARS
            || !ids.insert(candidate.candidate_id.as_str())
            || candidate.end_ms <= candidate.start_ms
            || !candidate.local_score.is_finite()
            || transcript_chars > MAX_TRANSCRIPT_CHARS
            || candidate.evidence.len() > MAX_EVIDENCE_ITEMS
            || candidate
                .evidence
                .iter()
                .any(|item| item.chars().count() > MAX_EVIDENCE_ITEM_CHARS)
        {
            return Err(smart_shorts_error(
                "fireworks_candidate_invalid",
                "GLM adaylarından biri geçersiz; yerel sonuçlar korunacak.",
                format!("invalid candidateId {:?}", candidate.candidate_id),
                false,
            ));
        }
    }
    if total_prompt_chars > MAX_TOTAL_PROMPT_CHARS {
        return Err(smart_shorts_error(
            "fireworks_prompt_too_large",
            "GLM aday metni güvenli istek sınırını aşıyor; yerel sonuçlar korunacak.",
            format!("Prompt contained {total_prompt_chars} characters"),
            false,
        ));
    }
    Ok(())
}

fn non_empty_secret(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.chars().count() <= MAX_SESSION_API_KEY_CHARS)
        .then(|| value.to_owned())
}

fn environment_api_key() -> Option<String> {
    std::env::var("FIREWORKS_API_KEY")
        .ok()
        .and_then(|value| non_empty_secret(&value))
        .or_else(project_env_api_key)
}

fn project_env_api_key() -> Option<String> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?;
    let path = project_root.join(".env");
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PROJECT_ENV_BYTES {
        return None;
    }
    let contents = fs::read_to_string(path).ok()?;
    dotenv_value(&contents, "FIREWORKS_API_KEY").and_then(|value| non_empty_secret(&value))
}

fn dotenv_value(contents: &str, expected_key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let (key, raw_value) = line.split_once('=')?;
        if key.trim() != expected_key {
            return None;
        }
        let raw_value = raw_value.trim();
        let value = if raw_value.len() >= 2
            && ((raw_value.starts_with('"') && raw_value.ends_with('"'))
                || (raw_value.starts_with('\'') && raw_value.ends_with('\'')))
        {
            &raw_value[1..raw_value.len() - 1]
        } else {
            raw_value
        };
        Some(value.to_owned())
    })
}

fn smart_shorts_error(
    code: impl Into<String>,
    user_message: impl Into<String>,
    technical_message: impl Into<String>,
    retryable: bool,
) -> RenderErrorReport {
    RenderErrorReport {
        code: code.into(),
        user_message: user_message.into(),
        technical_message: technical_message.into(),
        retryable,
        stderr_tail: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<SmartShortsCandidatePrompt> {
        vec![SmartShortsCandidatePrompt {
            candidate_id: "short:1000:31000".into(),
            start_ms: 1_000,
            end_ms: 31_000,
            local_score: 78.0,
            transcript: "Şimdi size inanılmaz bir şey göstereceğim.".into(),
            evidence: vec!["Güçlü açılış cümlesi".into()],
        }]
    }

    #[test]
    fn request_is_transcript_only_and_disables_storage() {
        let request = RankSmartShortsRequest {
            api_key: None,
            candidates: candidates(),
        };
        let body = build_request_body(&request);
        assert_eq!(body["store"], false);
        assert_eq!(body["model"], FIREWORKS_MODEL);
        assert_eq!(body["response_format"]["type"], "json_schema");
        let encoded = serde_json::to_string(&body).unwrap();
        assert!(encoded.contains("short:1000:31000"));
        assert!(!encoded.contains("sourcePath"));
    }

    #[test]
    fn response_keeps_only_known_unique_candidate_ids() {
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": serde_json::json!({
                        "rankings": [
                            { "candidateId": "short:1000:31000", "score": 91, "reason": "Net payoff", "focusTarget": "konuşan kişi" },
                            { "candidateId": "invented", "score": 100, "reason": "uydurma", "focusTarget": "?" },
                            { "candidateId": "short:1000:31000", "score": 20, "reason": "duplicate", "focusTarget": "kişi" }
                        ]
                    }).to_string()
                }
            }]
        });
        let parsed = parse_fireworks_response(&response.to_string(), &candidates()).unwrap();
        assert_eq!(parsed.rankings.len(), 1);
        assert_eq!(parsed.rankings[0].candidate_id, "short:1000:31000");
        assert_eq!(parsed.rankings[0].score, 91.0);
    }

    #[test]
    fn invalid_or_oversized_candidates_fail_closed() {
        let mut request = RankSmartShortsRequest {
            api_key: None,
            candidates: candidates(),
        };
        request.candidates[0].end_ms = request.candidates[0].start_ms;
        assert_eq!(
            validate_request(&request).unwrap_err().code,
            "fireworks_candidate_invalid"
        );

        let mut request = RankSmartShortsRequest {
            api_key: None,
            candidates: candidates(),
        };
        request.candidates[0].evidence = vec!["x".repeat(MAX_EVIDENCE_ITEM_CHARS + 1)];
        assert_eq!(
            validate_request(&request).unwrap_err().code,
            "fireworks_candidate_invalid"
        );
    }

    #[test]
    fn project_env_parser_keeps_the_key_server_side() {
        let contents = "# local only\nVITE_FIREWORKS_API_KEY=do-not-use\nexport FIREWORKS_API_KEY=\"server-secret\"\n";
        assert_eq!(
            dotenv_value(contents, "FIREWORKS_API_KEY").as_deref(),
            Some("server-secret")
        );
        assert!(non_empty_secret(&"x".repeat(MAX_SESSION_API_KEY_CHARS + 1)).is_none());
    }

    #[test]
    #[ignore = "performs one live, billable Fireworks GLM request using project .env"]
    fn real_fireworks_glm_smoke() {
        let api_key = environment_api_key().expect("FIREWORKS_API_KEY is required");
        let request = RankSmartShortsRequest {
            api_key: None,
            candidates: candidates(),
        };
        let response = rank_with_fireworks(&api_key, &request).expect("live Fireworks ranking");
        assert_eq!(response.model, FIREWORKS_MODEL);
        assert_eq!(response.rankings.len(), 1);
        assert_eq!(response.rankings[0].candidate_id, "short:1000:31000");
    }
}
