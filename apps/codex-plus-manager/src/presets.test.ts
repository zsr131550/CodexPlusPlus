import assert from "node:assert/strict";
import test from "node:test";

import presetData from "../../../assets/provider-presets.json" with { type: "json" };
import { PRESETS, createPresetPatch } from "./presets.ts";

test("PRESETS is the typed adapter over the shared JSON catalog", () => {
  assert.deepEqual(PRESETS, presetData);
  assert.equal(PRESETS[0]?.id, "openai");
  assert.equal(PRESETS.at(-1)?.id, "azure");
});

test("createPresetPatch retains the existing React patch shape", () => {
  const preset = PRESETS.find((candidate) => candidate.id === "deepseek");
  assert.ok(preset);

  assert.deepEqual(createPresetPatch(preset), {
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    upstreamBaseUrl: "https://api.deepseek.com",
    protocol: "chatCompletions",
    model: "deepseek-v4-flash",
    testModel: "deepseek-v4-flash",
    modelList: "deepseek-v4-flash\ndeepseek-v4-pro",
    relayMode: "pureApi",
    officialMixApiKey: false,
  });
});
