//! Rule-based, in-process prompt masking.
//!
//! When enabled, deterministic placeholders replace secrets in the outbound
//! JSON body before it leaves the machine; the same placeholders are restored
//! in the reply so the model never sees a real value and the user still does.
//! Placeholders are derived from the value, not random: a value maps to the
//! same placeholder for the whole request, which keeps tool calls and prompt
//! caches coherent.
//!
//! v1 handles keys/tokens, emails and configured custom words. It does not
//! try to recognise names or addresses (Magpie ships a ~1.6 GB local model for
//! that) and it does not restore placeholders inside tool-call argument
//! fragments — those are streamed as partial JSON and are masked outbound but
//! left masked in the reply. That limitation is stated in the UI.

use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

use super::pii_model::decoder::ModelSpan;
use std::hash::{DefaultHasher, Hash, Hasher};

/// Parsed privacy rules from settings JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrivacyRules {
    pub mask_secrets: bool,
    pub mask_emails: bool,
    pub custom_words: Vec<String>,
}

impl PrivacyRules {
    /// Parse the persisted JSON object. Unknown keys are ignored so a future
    /// setting can be added without breaking an older settings file.
    pub fn from_json(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self {
                mask_secrets: true,
                mask_emails: true,
                custom_words: Vec::new(),
            };
        };
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(value) else {
            return Self::default();
        };
        let flag = |key: &str, default: bool| {
            object
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(default)
        };
        let custom_words = object
            .get("custom_words")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|word| !word.trim().is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self {
            mask_secrets: flag("mask_secrets", true),
            mask_emails: flag("mask_emails", true),
            custom_words,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct PrivacyMaskAuditRow {
    pub occurred_at: String,
    pub platform: String,
    pub rule_kind: String,
    pub placeholder: String,
    pub direction: String,
    pub char_length: i64,
    pub prefix: String,
}

/// Read the newest audit rows, newest first. Placeholders only: no original
/// values are stored, so this endpoint cannot leak a secret.
pub async fn privacy_mask_insights(pool: &SqlitePool, limit: i64) -> Result<Vec<PrivacyMaskAuditRow>, crate::error::AppError> {
    let rows = sqlx::query_as::<_, PrivacyMaskAuditRow>(
        "SELECT occurred_at, platform, rule_kind, placeholder, direction, char_length, prefix
         FROM privacy_mask_audit ORDER BY occurred_at DESC, rowid DESC LIMIT ?",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
    .map_err(|error| crate::error::AppError::Database {
        code: "database.privacy_mask_insights",
        message: "Could not read masking audit".to_string(),
        details: Some(error.to_string()),
        recoverable: true,
    })?;
    Ok(rows)
}

/// A placeholder -> original-value map for one request.
pub type MaskRecords = HashMap<String, String>;

fn placeholder_for(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("⟦PM_{:016x}⟧", hasher.finish())
}

/// Longest secret-like token starting at `bytes[index]`, if any.
fn secret_at(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes.get(index) != Some(&b's') || bytes.get(index + 1) != Some(&b'k') {
        return None;
    }
    if bytes.get(index + 2) != Some(&b'-') {
        return None;
    }
    let mut end = index + 3;
    while end < bytes.len() {
        let byte = bytes[end];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            end += 1;
        } else {
            break;
        }
    }
    (end - index >= 16).then_some(end - index)
}

/// Longest email-like span starting at `bytes[index]`, if any.
fn email_at(bytes: &[u8], index: usize) -> Option<usize> {
    let mut end = index;
    while end < bytes.len() {
        let byte = bytes[end];
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-') {
            end += 1;
        } else {
            break;
        }
    }
    if end == index || bytes.get(end) != Some(&b'@') {
        return None;
    }
    let mut domain_end = end + 1;
    while domain_end < bytes.len() {
        let byte = bytes[domain_end];
        if byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' {
            domain_end += 1;
        } else {
            break;
        }
    }
    let domain = &bytes[end + 1..domain_end];
    let has_dot = domain.contains(&b'.');
    let has_tld = domain
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_alphabetic())
        .count()
        >= 2;
    (has_dot && has_tld).then_some(domain_end - index)
}

/// Replace each detected value with its stable placeholder, recording it.
pub fn mask_text(text: &str, rules: &PrivacyRules) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut records = HashMap::new();
    mask_text_with_records(text, rules, &mut records)
}

/// Same as [`mask_text`] but keeps the placeholder -> original map.
pub fn mask_text_with_records(text: &str, rules: &PrivacyRules, records: &mut MaskRecords) -> String {
    if text.is_empty() {
        return String::new();
    }
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        let span = if rules.mask_secrets {
            secret_at(bytes, index)
        } else {
            None
        }
        .or_else(|| {
            (rules.mask_emails && bytes[index].is_ascii_alphanumeric())
                .then(|| email_at(bytes, index))
                .flatten()
        });
        if let Some(length) = span {
            let original = &text[index..index + length];
            let placeholder = placeholder_for(original);
            records
                .entry(placeholder.clone())
                .or_insert_with(|| original.to_string());
            output.push_str(&placeholder);
            index += length;
            continue;
        }
        let ch = text[index..].chars().next().expect("non-empty suffix");
        output.push(ch);
        index += ch.len_utf8();
    }
    if !rules.custom_words.is_empty() {
        for word in &rules.custom_words {
            if word.is_empty() || !output.contains(word) {
                continue;
            }
            let placeholder = placeholder_for(word);
            records
                .entry(placeholder.clone())
                .or_insert_with(|| word.clone());
            output = output.replace(word, &placeholder);
        }
    }
    output
}

/// Apply local-model spans first, then the rule scanner. Model spans are byte
/// offsets in the original text and are replaced right-to-left so earlier
/// offsets stay valid. Both kinds of detection share the same placeholder and
/// record map, which keeps streaming/buffered restoration unchanged.
pub fn mask_text_with_spans(
    text: &str,
    rules: &PrivacyRules,
    spans: &[ModelSpan],
    records: &mut MaskRecords,
) -> String {
    if spans.is_empty() {
        return mask_text_with_records(text, rules, records);
    }
    let mut buffer = text.to_string();
    let mut sorted: Vec<&ModelSpan> = spans.iter().collect();
    sorted.sort_by_key(|span| std::cmp::Reverse(span.start));
    for span in sorted {
        if span.start >= span.end
            || span.end > buffer.len()
            || !buffer.is_char_boundary(span.start)
            || !buffer.is_char_boundary(span.end)
        {
            continue;
        }
        let original = buffer[span.start..span.end].to_string();
        let placeholder = placeholder_for(&original);
        records
            .entry(placeholder.clone())
            .or_insert_with(|| original.clone());
        buffer.replace_range(span.start..span.end, &placeholder);
    }
    mask_text_with_records(&buffer, rules, records)
}

/// Mask a JSON body while letting the caller contribute model-detected spans
/// keyed by JSON pointer. Returns the masked bytes, placeholder -> original
/// records, and placeholder -> model label for audit classification.
pub fn mask_body_with_detector<F>(
    body: &[u8],
    rules: &PrivacyRules,
    detector: F,
) -> Result<(Vec<u8>, MaskRecords, HashMap<String, String>), serde_json::Error>
where
    F: Fn(&str, &str) -> Vec<ModelSpan>,
{
    let value: Value = serde_json::from_slice(body)?;
    let mut records = HashMap::new();
    let mut labels = HashMap::new();
    let masked = mask_value_with_detector(value, "", rules, &detector, &mut records, &mut labels);
    Ok((serde_json::to_vec(&masked)?, records, labels))
}

fn json_pointer_segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn mask_value_with_detector<F>(
    value: Value,
    path: &str,
    rules: &PrivacyRules,
    detector: &F,
    records: &mut MaskRecords,
    labels: &mut HashMap<String, String>,
) -> Value
where
    F: Fn(&str, &str) -> Vec<ModelSpan>,
{
    match value {
        Value::String(text) => {
            let spans = detector(path, &text);
            for span in &spans {
                if let Some(original) = text.get(span.start..span.end) {
                    labels.insert(placeholder_for(original), span.label.clone());
                }
            }
            Value::String(mask_text_with_spans(&text, rules, &spans, records))
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    mask_value_with_detector(
                        item,
                        &format!("{path}/{index}"),
                        rules,
                        detector,
                        records,
                        labels,
                    )
                })
                .collect(),
        ),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, item)| {
                    let child = format!("{path}/{}", json_pointer_segment(&key));
                    (
                        key,
                        mask_value_with_detector(item, &child, rules, detector, records, labels),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

/// Collect `(json_pointer, string_value)` pairs in traversal order so a
/// detector can be called once per request with a stable path index.
pub fn collect_json_strings(body: &[u8]) -> Result<Vec<(String, String)>, serde_json::Error> {
    let value: Value = serde_json::from_slice(body)?;
    let mut out = Vec::new();
    collect_json_strings_inner(value, "", &mut out);
    Ok(out)
}

fn collect_json_strings_inner(value: Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(text) => out.push((path.to_string(), text)),
        Value::Array(items) => {
            for (index, item) in items.into_iter().enumerate() {
                collect_json_strings_inner(item, &format!("{path}/{index}"), out);
            }
        }
        Value::Object(entries) => {
            for (key, item) in entries {
                collect_json_strings_inner(item, &format!("{path}/{}", json_pointer_segment(&key)), out);
            }
        }
        _ => {}
    }
}

/// Restore placeholders in streamed text. A placeholder is never partly
/// replaced: an incomplete tail is left untouched for the next chunk (the SSE
/// framer guarantees complete records, so in practice this only matters for
/// tool-call argument fragments).
pub fn restore_placeholders(text: &str, records: &MaskRecords) -> String {
    if records.is_empty() {
        return text.to_string();
    }
    let mut output = text.to_string();
    for (placeholder, original) in records {
        output = output.replace(placeholder, original);
    }
    output
}

/// Mask every string value in a JSON request document.
pub fn mask_body(body: &[u8], rules: &PrivacyRules) -> Result<(Vec<u8>, MaskRecords), serde_json::Error> {
    let value: Value = serde_json::from_slice(body)?;
    let mut records = HashMap::new();
    let masked = mask_value(value, rules, &mut records);
    Ok((serde_json::to_vec(&masked)?, records))
}

fn mask_value(value: Value, rules: &PrivacyRules, records: &mut MaskRecords) -> Value {
    match value {
        Value::String(text) => Value::String(mask_text_with_records(&text, rules, records)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| mask_value(item, rules, records))
                .collect(),
        ),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, item)| (key, mask_value(item, rules, records)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> PrivacyRules {
        PrivacyRules {
            mask_secrets: true,
            mask_emails: true,
            custom_words: vec!["ProjectApollo".to_string()],
        }
    }

    #[test]
    fn placeholders_are_deterministic_and_restorable() {
        let original = "key sk-abcdefghijklmnop and a@b.com plus ProjectApollo";
        let mut records = HashMap::new();
        let masked = mask_text_with_records(original, &rules(), &mut records);
        assert!(!masked.contains("sk-abcdefghijklmnop"));
        assert!(!masked.contains("a@b.com"));
        assert!(!masked.contains("ProjectApollo"));
        assert_eq!(records.len(), 3);
        assert_eq!(restore_placeholders(&masked, &records), original);

        let again = mask_text(original, &rules());
        assert_eq!(again, masked);
    }

    #[test]
    fn a_short_or_malformed_secret_is_left_alone() {
        let text = "sk-short and user@localhost";
        let masked = mask_text(text, &rules());
        assert_eq!(masked, text);
    }

    #[test]
    fn json_body_masks_nested_strings_only() {
        let body = br#"{"model":"m","messages":[{"content":"sk-abcdefghijklmnop"}],"stream":true}"#;
        let (masked, records) = mask_body(body, &rules()).expect("mask");
        let value: Value = serde_json::from_slice(&masked).expect("json");
        assert_eq!(value["model"], "m");
        assert_eq!(value["stream"], true);
        assert!(!value["messages"][0]["content"].as_str().unwrap().contains("sk-abcdefghijklmnop"));
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn model_spans_and_rules_share_the_placeholder_path() {
        let text = "联系人张三 sk-abcdefghijklmnop";
        let bytes = text.find("张三").expect("found");
        let span = ModelSpan {
            start: bytes,
            end: bytes + "张三".len(),
            label: "private_person".to_string(),
            score: 0.9,
        };
        let mut records = HashMap::new();
        let masked = mask_text_with_spans(text, &rules(), &[span], &mut records);
        assert!(!masked.contains("张三"));
        assert!(!masked.contains("sk-abcdefghijklmnop"));
        assert_eq!(restore_placeholders(&masked, &records), text);
    }

    #[test]
    fn rules_default_to_masking_secrets_and_emails() {
        let rules = PrivacyRules::from_json(Some(r#"{"custom_words":["X"]}"#));
        assert!(rules.mask_secrets);
        assert!(rules.mask_emails);
        assert_eq!(rules.custom_words, vec!["X"]);
    }
}
