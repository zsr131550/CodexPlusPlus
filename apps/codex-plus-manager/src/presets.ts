/**
 * Codex++ provider presets.
 * Based on the cc-switch (MIT) codexProviderPresets catalog by Jason Young.
 */

import presetData from "../../../assets/provider-presets.json" with { type: "json" };

export type PresetCategory = "official" | "aggregator" | "third_party" | "cn_official";

export type RelayProtocol = "responses" | "chatCompletions";

export interface ProviderPreset {
  id: string;
  name: string;
  websiteUrl?: string;
  apiKeyUrl?: string;
  category: PresetCategory;
  baseUrl: string;
  protocol: RelayProtocol;
  model: string;
  modelList?: string[];
}

export interface ProviderPresetPatch {
  name: string;
  baseUrl: string;
  upstreamBaseUrl: string;
  protocol: RelayProtocol;
  model: string;
  testModel: string;
  modelList: string;
  relayMode: "official" | "pureApi";
  officialMixApiKey: false;
}

export const PRESETS: ProviderPreset[] = presetData as ProviderPreset[];

export function createPresetPatch(preset: ProviderPreset): ProviderPresetPatch {
  return {
    name: preset.name,
    baseUrl: preset.baseUrl,
    upstreamBaseUrl: preset.baseUrl,
    protocol: preset.protocol,
    model: preset.model,
    testModel: preset.model,
    modelList: preset.modelList?.join("\n") ?? "",
    relayMode: preset.category === "official" ? "official" : "pureApi",
    officialMixApiKey: false,
  };
}
