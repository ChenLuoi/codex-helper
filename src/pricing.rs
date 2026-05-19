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

const MODEL_PRICING: &[ModelPricing] = &[
    ModelPricing {
        key: "gpt-5.5",
        label: "GPT-5.5",
        input_credits_per_million: 125.0,
        cached_input_credits_per_million: 12.5,
        output_credits_per_million: 750.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-5.4",
        label: "GPT-5.4",
        input_credits_per_million: 62.5,
        cached_input_credits_per_million: 6.25,
        output_credits_per_million: 375.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-5.4-mini",
        label: "GPT-5.4-mini",
        input_credits_per_million: 18.75,
        cached_input_credits_per_million: 1.875,
        output_credits_per_million: 113.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-5.3-codex",
        label: "GPT-5.3-Codex",
        input_credits_per_million: 43.75,
        cached_input_credits_per_million: 4.375,
        output_credits_per_million: 350.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-5.2",
        label: "GPT-5.2",
        input_credits_per_million: 43.75,
        cached_input_credits_per_million: 4.375,
        output_credits_per_million: 350.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-5.3-codex-spark",
        label: "GPT-5.3-Codex-Spark",
        input_credits_per_million: 0.0,
        cached_input_credits_per_million: 0.0,
        output_credits_per_million: 0.0,
        note: Some("research preview; charged at 0 credits"),
    },
    ModelPricing {
        key: "gpt-image-2 (image)",
        label: "GPT-Image-2 (image)",
        input_credits_per_million: 200.0,
        cached_input_credits_per_million: 50.0,
        output_credits_per_million: 750.0,
        note: None,
    },
    ModelPricing {
        key: "gpt-image-2 (text)",
        label: "GPT-Image-2 (text)",
        input_credits_per_million: 125.0,
        cached_input_credits_per_million: 31.25,
        output_credits_per_million: 250.0,
        note: None,
    },
];

pub const CODEX_RATE_CARD_SOURCE: RateCardSource = RateCardSource {
    name: "OpenAI Help Center Codex rate card",
    checked_at: "2026-05-13",
    credit_to_usd: "25 credits = $1",
};

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
    MODEL_PRICING
        .iter()
        .copied()
        .find(|pricing| pricing.key == key)
}

pub fn list_model_pricing() -> Vec<ModelPricing> {
    let mut pricing = MODEL_PRICING.to_vec();
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
}
