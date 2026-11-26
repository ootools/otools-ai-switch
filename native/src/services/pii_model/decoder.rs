//! BIO decoding for AstrLink Guard's token-classification output.
//!
//! The model emits one logit vector per token, not a chat reply. Decoding
//! mirrors the publisher's reference: softmax, argmax, skip special tokens,
//! span score = minimum token probability, no Viterbi.

/// A detected span in **byte** offsets of the original text.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelSpan {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct CharSpan {
    start: usize,
    end: usize,
    label: String,
    score: f32,
}

/// Convert tokenizer character offsets to byte offsets. CJK and emoji are the
/// reason this cannot be a no-op: one char can span several bytes.
pub fn char_offsets_to_byte_offsets(text: &str, spans: &[ModelSpan]) -> Vec<ModelSpan> {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();
    spans
        .iter()
        .filter_map(|span| {
            let start = *boundaries.get(span.start)?;
            let end = *boundaries.get(span.end)?;
            if start >= end {
                return None;
            }
            Some(ModelSpan {
                start,
                end,
                label: span.label.clone(),
                score: span.score,
            })
        })
        .collect()
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return vec![0.0; logits.len()];
    }
    let exps: Vec<f32> = logits.iter().map(|value| (value - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum <= 0.0 {
        return vec![0.0; logits.len()];
    }
    exps.into_iter().map(|value| value / sum).collect()
}

/// Decode one window of `[token_count, labels.len()]` flattened logits.
/// `offsets` are tokenizer **character** offsets; special tokens use
/// `start == end` and are skipped.
pub fn decode_window(
    logits: &[f32],
    token_count: usize,
    offsets: &[(usize, usize)],
    labels: &[String],
    threshold: f32,
) -> Vec<ModelSpan> {
    if labels.is_empty() || token_count == 0 || logits.len() < token_count * labels.len() {
        return Vec::new();
    }
    let mut current: Option<CharSpan> = None;
    let mut spans = Vec::new();
    for token in 0..token_count {
        let offset = offsets.get(token).copied().unwrap_or((0, 0));
        if offset.0 == offset.1 {
            continue;
        }
        let row = &logits[token * labels.len()..(token + 1) * labels.len()];
        let probs = softmax(row);
        let (best, probability) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, value)| (index, *value))
            .unwrap_or((0, 0.0));
        let tag = labels.get(best).map(String::as_str).unwrap_or("O");
        if tag == "O" {
            if let Some(span) = current.take() {
                spans.push(span);
            }
            continue;
        }
        let (prefix, kind) = tag.split_once('-').unwrap_or((tag, tag));
        let open_new = match current.as_ref() {
            None => true,
            Some(span) => prefix == "B" || span.label != kind,
        };
        if open_new {
            if let Some(span) = current.take() {
                spans.push(span);
            }
            current = Some(CharSpan {
                start: offset.0,
                end: offset.1,
                label: kind.to_string(),
                score: probability,
            });
        } else if let Some(span) = current.as_mut() {
            span.end = offset.1;
            span.score = span.score.min(probability);
        }
    }
    if let Some(span) = current {
        spans.push(span);
    }
    spans
        .into_iter()
        .filter(|span| span.score >= threshold)
        .map(|span| ModelSpan {
            start: span.start,
            end: span.end,
            label: span.label,
            score: span.score,
        })
        .collect()
}

/// Merge spans from overlapping 512-token windows. Exact duplicates collapse;
/// a shorter span fully contained in a longer one with a lower score is kept
/// only when it carries a different label (the more specific detection).
pub fn merge_windows(spans: Vec<ModelSpan>) -> Vec<ModelSpan> {
    let mut sorted = spans;
    sorted.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| b.end.cmp(&a.end))
            .then_with(|| b.score.total_cmp(&a.score))
    });
    let mut kept: Vec<ModelSpan> = Vec::new();
    for span in sorted {
        let contained = kept.iter().any(|existing| {
            existing.start <= span.start
                && existing.end >= span.end
                && (existing.label == span.label || existing.score >= span.score)
        });
        if !contained {
            kept.push(span);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> Vec<String> {
        vec![
            "O".to_string(),
            "B-person".to_string(),
            "I-person".to_string(),
            "B-email".to_string(),
        ]
    }

    #[test]
    fn decodes_a_bio_person_span_with_minimum_probability() {
        // tokens: [special(skip), B-person, I-person, O]
        let logits = vec![
            0.0, 0.0, 0.0, 0.0, // skipped
            0.1, 3.0, 0.0, 0.0, // B-person ~0.9
            0.1, 0.0, 2.0, 0.0, // I-person ~0.88
            4.0, 0.0, 0.0, 0.0, // O
        ];
        let offsets = vec![(0, 0), (0, 1), (1, 2), (2, 2)];
        let spans = decode_window(&logits, 4, &offsets, &labels(), 0.5);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].label, "person");
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].end, 2);
        assert!(spans[0].score >= 0.5 && spans[0].score <= 1.0);
    }

    #[test]
    fn a_low_confidence_span_is_dropped() {
        let logits = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.1, 0.0, 0.2];
        let offsets = vec![(0, 0), (0, 1)];
        assert!(decode_window(&logits, 2, &offsets, &labels(), 0.8).is_empty());
    }

    #[test]
    fn char_offsets_map_through_cjk_and_emoji() {
        let text = "张🙂三";
        let spans = vec![ModelSpan {
            start: 0,
            end: 3,
            label: "person".to_string(),
            score: 0.9,
        }];
        let bytes = char_offsets_to_byte_offsets(text, &spans);
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0].start, 0);
        assert_eq!(bytes[0].end, text.len());
        assert_eq!(&text[bytes[0].start..bytes[0].end], text);
    }

    #[test]
    fn merge_drops_contained_duplicates() {
        let span = |start, end, score| ModelSpan {
            start,
            end,
            label: "person".to_string(),
            score,
        };
        let merged = merge_windows(vec![span(0, 4, 0.9), span(0, 4, 0.7), span(10, 14, 0.95)]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].score, 0.9);
    }
}
