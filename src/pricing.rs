use serde::Deserialize;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub key: &'static str,
    pub label: &'static str,
    pub input_credits_per_million: f64,
    pub cached_input_credits_per_million: f64,
    pub output_credits_per_million: f64,
    pub note: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateCardSource {
    pub name: &'static str,
    pub checked_at: &'static str,
    pub credit_to_usd: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreditCost {
    pub priced: bool,
    pub pricing_label: String,
    pub unpriced_reason: Option<String>,
    pub billable_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub credits: f64,
}

const RATE_CARD_JSON: &str = include_str!("../data/codex-rate-card.json");

static RATE_CARD: LazyLock<RateCard> = LazyLock::new(load_rate_card);
pub static CODEX_RATE_CARD_SOURCE: LazyLock<RateCardSource> = LazyLock::new(|| rate_card().source);

#[derive(Debug, Clone)]
struct RateCard {
    source: RateCardSource,
    models: Vec<ModelPricing>,
}

#[derive(Debug, Deserialize)]
struct RawRateCard {
    source: RawRateCardSource,
    models: Vec<RawModelPricing>,
}

#[derive(Debug, Deserialize)]
struct RawRateCardSource {
    name: String,
    checked_at: String,
    credit_to_usd: String,
}

#[derive(Debug, Deserialize)]
struct RawModelPricing {
    key: String,
    label: String,
    input_credits_per_million: f64,
    cached_input_credits_per_million: f64,
    output_credits_per_million: f64,
    note: Option<String>,
}

pub fn normalize_model_name(model: &str) -> String {
    model
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn pricing_key_for_model(model: &str) -> String {
    let normalized = normalize_model_name(model);
    match normalized.as_str() {
        "gpt-5.4 mini" => "gpt-5.4-mini".to_string(),
        "gpt-5.3 codex" => "gpt-5.3-codex".to_string(),
        "gpt-image-2:image"
        | "gpt-image-2-image"
        | "gpt-image-2 image"
        | "gpt-image-2.0:image"
        | "gpt-image-2.0-image"
        | "gpt-image-2.0 image"
        | "gpt-image-2.0 (image)" => "gpt-image-2 (image)".to_string(),
        "gpt-image-2:text"
        | "gpt-image-2-text"
        | "gpt-image-2 text"
        | "gpt-image-2.0:text"
        | "gpt-image-2.0-text"
        | "gpt-image-2.0 text"
        | "gpt-image-2.0 (text)" => "gpt-image-2 (text)".to_string(),
        _ => normalized,
    }
}

pub fn get_model_pricing(model: &str) -> Option<ModelPricing> {
    let key = pricing_key_for_model(model);
    rate_card()
        .models
        .iter()
        .copied()
        .find(|pricing| pricing.key == key)
}

pub fn list_model_pricing() -> Vec<ModelPricing> {
    let mut pricing = rate_card().models.clone();
    pricing.sort_by(|left, right| left.key.cmp(right.key));
    pricing
}

pub fn list_known_unpriced_models() -> Vec<ModelPricing> {
    Vec::new()
}

pub fn calculate_credit_cost(model: &str, usage: TokenUsage) -> CreditCost {
    let cached_input_tokens = usage.cached_input_tokens.min(usage.input_tokens);
    let billable_input_tokens = usage.input_tokens.saturating_sub(cached_input_tokens);
    let pricing = get_model_pricing(model);

    match pricing {
        Some(pricing) => CreditCost {
            priced: true,
            pricing_label: pricing.label.to_string(),
            unpriced_reason: None,
            billable_input_tokens,
            cached_input_tokens,
            output_tokens: usage.output_tokens,
            credits: (billable_input_tokens as f64 * pricing.input_credits_per_million
                + cached_input_tokens as f64 * pricing.cached_input_credits_per_million
                + usage.output_tokens as f64 * pricing.output_credits_per_million)
                / 1_000_000.0,
        },
        None => CreditCost {
            priced: false,
            pricing_label: model.to_string(),
            unpriced_reason: None,
            billable_input_tokens,
            cached_input_tokens,
            output_tokens: usage.output_tokens,
            credits: 0.0,
        },
    }
}

fn rate_card() -> &'static RateCard {
    &RATE_CARD
}

fn load_rate_card() -> RateCard {
    let raw: RawRateCard = serde_json::from_str(RATE_CARD_JSON).unwrap_or_else(|error| {
        panic!("Failed to parse data/codex-rate-card.json: {error}");
    });
    validate_rate_card(&raw);

    RateCard {
        source: RateCardSource {
            name: leak_str(raw.source.name),
            checked_at: leak_str(raw.source.checked_at),
            credit_to_usd: leak_str(raw.source.credit_to_usd),
        },
        models: raw
            .models
            .into_iter()
            .map(|model| ModelPricing {
                key: leak_str(model.key),
                label: leak_str(model.label),
                input_credits_per_million: model.input_credits_per_million,
                cached_input_credits_per_million: model.cached_input_credits_per_million,
                output_credits_per_million: model.output_credits_per_million,
                note: model.note.map(leak_str),
            })
            .collect(),
    }
}

fn validate_rate_card(raw: &RawRateCard) {
    assert_non_empty(&raw.source.name, "source.name");
    assert_non_empty(&raw.source.checked_at, "source.checked_at");
    assert_non_empty(&raw.source.credit_to_usd, "source.credit_to_usd");

    if raw.models.is_empty() {
        panic!("data/codex-rate-card.json must define at least one model");
    }

    let mut keys = HashSet::new();
    for model in &raw.models {
        assert_non_empty(&model.key, "models[].key");
        assert_non_empty(&model.label, "models[].label");
        if !keys.insert(model.key.as_str()) {
            panic!(
                "data/codex-rate-card.json has duplicate model key: {}",
                model.key
            );
        }
        assert_non_negative_finite(
            model.input_credits_per_million,
            "models[].input_credits_per_million",
        );
        assert_non_negative_finite(
            model.cached_input_credits_per_million,
            "models[].cached_input_credits_per_million",
        );
        assert_non_negative_finite(
            model.output_credits_per_million,
            "models[].output_credits_per_million",
        );
    }
}

fn assert_non_empty(value: &str, path: &str) {
    if value.trim().is_empty() {
        panic!("data/codex-rate-card.json field {path} cannot be empty");
    }
}

fn assert_non_negative_finite(value: f64, path: &str) {
    if !value.is_finite() || value < 0.0 {
        panic!("data/codex-rate-card.json field {path} must be finite and non-negative");
    }
}

fn leak_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_model_names_and_aliases() {
        assert_eq!(normalize_model_name("  GPT-5.4   MINI "), "gpt-5.4 mini");
        assert_eq!(pricing_key_for_model("GPT-5.4   MINI"), "gpt-5.4-mini");
        assert_eq!(
            get_model_pricing("gpt-image-2.0:image")
                .expect("image pricing")
                .label,
            "GPT-Image-2 (image)"
        );
    }

    #[test]
    fn calculates_credit_cost_from_billable_cached_and_output_tokens() {
        let cost = calculate_credit_cost(
            "gpt-5.5",
            TokenUsage {
                input_tokens: 1000,
                cached_input_tokens: 200,
                output_tokens: 300,
            },
        );

        assert!(cost.priced);
        assert_eq!(cost.pricing_label, "GPT-5.5");
        assert_eq!(cost.billable_input_tokens, 800);
        assert_eq!(cost.cached_input_tokens, 200);
        assert_eq!(cost.output_tokens, 300);
        assert!((cost.credits - 0.3275).abs() < 0.000001);
    }

    #[test]
    fn clamps_cached_input_and_handles_unknown_models() {
        let cost = calculate_credit_cost(
            "future-model",
            TokenUsage {
                input_tokens: 100,
                cached_input_tokens: 250,
                output_tokens: 50,
            },
        );

        assert!(!cost.priced);
        assert_eq!(cost.pricing_label, "future-model");
        assert_eq!(cost.billable_input_tokens, 0);
        assert_eq!(cost.cached_input_tokens, 100);
        assert_eq!(cost.credits, 0.0);
    }

    #[test]
    fn spark_model_is_priced_at_zero_credits() {
        let cost = calculate_credit_cost(
            "gpt-5.3-codex-spark",
            TokenUsage {
                input_tokens: 500,
                cached_input_tokens: 0,
                output_tokens: 100,
            },
        );

        assert!(cost.priced);
        assert_eq!(cost.pricing_label, "GPT-5.3-Codex-Spark");
        assert_eq!(cost.credits, 0.0);
    }

    #[test]
    fn pricing_inventory_is_sorted() {
        let keys = list_model_pricing()
            .into_iter()
            .map(|pricing| pricing.key)
            .collect::<Vec<_>>();

        assert_eq!(keys.first(), Some(&"gpt-5.2"));
        assert!(keys.contains(&"gpt-5.5"));
    }

    #[test]
    fn loads_source_metadata_from_static_rate_card() {
        assert_eq!(
            CODEX_RATE_CARD_SOURCE.name,
            "OpenAI Help Center Codex rate card"
        );
        assert_eq!(CODEX_RATE_CARD_SOURCE.checked_at, "2026-05-13");
        assert_eq!(CODEX_RATE_CARD_SOURCE.credit_to_usd, "25 credits = $1");
        assert_eq!(list_model_pricing().len(), 8);
    }
}
