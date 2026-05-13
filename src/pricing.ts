import type { TokenUsage } from "./stats.js";

export type ModelPricing = {
  label: string;
  inputCreditsPerMillion: number;
  cachedInputCreditsPerMillion: number;
  outputCreditsPerMillion: number;
  note?: string;
};

export type KnownUnpricedModel = {
  label: string;
  note: string;
};

export type CreditCost = {
  priced: boolean;
  pricingLabel: string;
  unpricedReason?: string;
  billableInputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  credits: number;
};

export const CODEX_RATE_CARD_SOURCE = {
  name: "OpenAI Help Center Codex rate card",
  url: "https://help.openai.com/en/articles/20001106-codex-rate-card",
  checkedAt: "2026-05-13",
  creditToUsd: "25 credits = $1"
} as const;

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
  "gpt-5.3-codex-spark": {
    label: "GPT-5.3-Codex-Spark",
    inputCreditsPerMillion: 0,
    cachedInputCreditsPerMillion: 0,
    outputCreditsPerMillion: 0,
    note: "research preview; charged at 0 credits"
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

export const KNOWN_UNPRICED_MODELS: Record<string, KnownUnpricedModel> = {};

const MODEL_ALIASES: Record<string, string> = {
  "gpt-5.3-codex-spark": "gpt-5.3-codex-spark",
  "gpt-5.4 mini": "gpt-5.4-mini",
  "gpt-5.3 codex": "gpt-5.3-codex",
  "gpt-image-2:image": "gpt-image-2 (image)",
  "gpt-image-2-image": "gpt-image-2 (image)",
  "gpt-image-2 image": "gpt-image-2 (image)",
  "gpt-image-2.0:image": "gpt-image-2 (image)",
  "gpt-image-2.0-image": "gpt-image-2 (image)",
  "gpt-image-2.0 image": "gpt-image-2 (image)",
  "gpt-image-2.0 (image)": "gpt-image-2 (image)",
  "gpt-image-2:text": "gpt-image-2 (text)",
  "gpt-image-2-text": "gpt-image-2 (text)",
  "gpt-image-2 text": "gpt-image-2 (text)",
  "gpt-image-2.0:text": "gpt-image-2 (text)",
  "gpt-image-2.0-text": "gpt-image-2 (text)",
  "gpt-image-2.0 text": "gpt-image-2 (text)",
  "gpt-image-2.0 (text)": "gpt-image-2 (text)"
};

export function getModelPricing(model: string): ModelPricing | undefined {
  const key = pricingKeyForModel(model);
  return MODEL_PRICING[key];
}

export function getKnownUnpricedModel(model: string): KnownUnpricedModel | undefined {
  return KNOWN_UNPRICED_MODELS[pricingKeyForModel(model)];
}

export function listModelPricing() {
  return Object.entries(MODEL_PRICING)
    .map(([key, pricing]) => ({ key, ...pricing }))
    .sort((left, right) => left.key.localeCompare(right.key));
}

export function listKnownUnpricedModels() {
  return Object.entries(KNOWN_UNPRICED_MODELS)
    .map(([key, model]) => ({ key, ...model }))
    .sort((left, right) => left.key.localeCompare(right.key));
}

export function calculateCreditCost(model: string, usage: TokenUsage): CreditCost {
  const pricing = getModelPricing(model);
  const unpriced = getKnownUnpricedModel(model);
  const cachedInputTokens = Math.max(0, Math.min(usage.cachedInputTokens, usage.inputTokens));
  const billableInputTokens = Math.max(0, usage.inputTokens - cachedInputTokens);

  if (pricing === undefined) {
    return {
      priced: false,
      pricingLabel: unpriced?.label ?? model,
      unpricedReason: unpriced?.note,
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

function pricingKeyForModel(model: string) {
  const normalized = normalizeModelName(model);
  return MODEL_ALIASES[normalized] ?? normalized;
}
