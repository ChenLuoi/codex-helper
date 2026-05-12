import type { TokenUsage } from "./stats.js";

export type ModelPricing = {
  label: string;
  inputCreditsPerMillion: number;
  cachedInputCreditsPerMillion: number;
  outputCreditsPerMillion: number;
  note?: string;
};

export type CreditCost = {
  priced: boolean;
  pricingLabel: string;
  billableInputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  credits: number;
};

export const MODEL_PRICING: Record<string, ModelPricing> = {
  "gpt-5.5": {
    label: "GPT-5.5",
    inputCreditsPerMillion: 125,
    cachedInputCreditsPerMillion: 12.5,
    outputCreditsPerMillion: 750
  },
  "gpt-5.4": {
    label: "GPT-5.4",
    inputCreditsPerMillion: 62.5,
    cachedInputCreditsPerMillion: 6.25,
    outputCreditsPerMillion: 375
  },
  "gpt-5.4-mini": {
    label: "GPT-5.4-mini",
    inputCreditsPerMillion: 18.75,
    cachedInputCreditsPerMillion: 1.875,
    outputCreditsPerMillion: 113
  },
  "gpt-5.3-codex": {
    label: "GPT-5.3-Codex",
    inputCreditsPerMillion: 43.75,
    cachedInputCreditsPerMillion: 4.375,
    outputCreditsPerMillion: 350
  },
  "gpt-5.2": {
    label: "GPT-5.2",
    inputCreditsPerMillion: 43.75,
    cachedInputCreditsPerMillion: 4.375,
    outputCreditsPerMillion: 350
  },
  "gpt-image-2 (image)": {
    label: "GPT-Image-2 (image)",
    inputCreditsPerMillion: 200,
    cachedInputCreditsPerMillion: 50,
    outputCreditsPerMillion: 750
  },
  "gpt-image-2 (text)": {
    label: "GPT-Image-2 (text)",
    inputCreditsPerMillion: 125,
    cachedInputCreditsPerMillion: 31.25,
    outputCreditsPerMillion: 250
  }
};

const MODEL_ALIASES: Record<string, string> = {
  "gpt-5.3-codex-spark": "gpt-5.3-codex-spark",
  "gpt-5.4 mini": "gpt-5.4-mini",
  "gpt-5.3 codex": "gpt-5.3-codex",
  "gpt-image-2:image": "gpt-image-2 (image)",
  "gpt-image-2-image": "gpt-image-2 (image)",
  "gpt-image-2 image": "gpt-image-2 (image)",
  "gpt-image-2:text": "gpt-image-2 (text)",
  "gpt-image-2-text": "gpt-image-2 (text)",
  "gpt-image-2 text": "gpt-image-2 (text)"
};

export function getModelPricing(model: string): ModelPricing | undefined {
  const normalized = normalizeModelName(model);
  const key = MODEL_ALIASES[normalized] ?? normalized;
  return MODEL_PRICING[key];
}

export function calculateCreditCost(model: string, usage: TokenUsage): CreditCost {
  const pricing = getModelPricing(model);
  const cachedInputTokens = Math.max(0, Math.min(usage.cachedInputTokens, usage.inputTokens));
  const billableInputTokens = Math.max(0, usage.inputTokens - cachedInputTokens);

  if (pricing === undefined) {
    return {
      priced: false,
      pricingLabel: model,
      billableInputTokens,
      cachedInputTokens,
      outputTokens: usage.outputTokens,
      credits: 0
    };
  }

  return {
    priced: true,
    pricingLabel: pricing.label,
    billableInputTokens,
    cachedInputTokens,
    outputTokens: usage.outputTokens,
    credits:
      (billableInputTokens * pricing.inputCreditsPerMillion +
        cachedInputTokens * pricing.cachedInputCreditsPerMillion +
        usage.outputTokens * pricing.outputCreditsPerMillion) /
      1_000_000
  };
}

export function normalizeModelName(model: string) {
  return model.trim().toLowerCase().replace(/\s+/g, " ");
}
