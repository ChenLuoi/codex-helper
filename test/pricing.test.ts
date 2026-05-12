import { describe, expect, it } from "vitest";
import { calculateCreditCost, getModelPricing, normalizeModelName } from "../src/pricing.js";

describe("pricing", () => {
  it("normalizes model names and resolves aliases", () => {
    expect(normalizeModelName(" GPT-5.3 Codex ")).toBe("gpt-5.3 codex");
    expect(getModelPricing("GPT-5.3 Codex")?.label).toBe("GPT-5.3-Codex");
    expect(getModelPricing("gpt-image-2:image")?.label).toBe("GPT-Image-2 (image)");
  });

  it("calculates credits from non-cached input, cached input, and output", () => {
    const cost = calculateCreditCost("gpt-5.5", {
      inputTokens: 1_000_000,
      cachedInputTokens: 400_000,
      outputTokens: 200_000,
      reasoningOutputTokens: 50_000,
      totalTokens: 1_200_000
    });

    expect(cost.priced).toBe(true);
    expect(cost.billableInputTokens).toBe(600_000);
    expect(cost.cachedInputTokens).toBe(400_000);
    expect(cost.credits).toBeCloseTo(230);
  });

  it("leaves research-preview or unknown models unpriced", () => {
    const cost = calculateCreditCost("gpt-5.3-codex-spark", {
      inputTokens: 1_000_000,
      cachedInputTokens: 0,
      outputTokens: 1_000_000,
      reasoningOutputTokens: 0,
      totalTokens: 2_000_000
    });

    expect(cost.priced).toBe(false);
    expect(cost.credits).toBe(0);
  });
});
