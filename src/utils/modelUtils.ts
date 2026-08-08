import type { AppConfig, VirtualModel, ParameterOverrides, ProviderProtocol } from "../types/config";
import type { ReasoningLevel } from "../types/reasoning";
import { t } from "../i18n";

export function effectiveHostModelId(model: VirtualModel): string {
  if (model.host_model_id) return model.host_model_id;
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(model.id)) {
    hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  }
  return `MODEL_PLACEHOLDER_M${400 + (hash % 200)}`;
}

function virtualModelCatalogKey(model: VirtualModel): string {
  return model.id.startsWith("custom-") ? model.id : `custom-${model.id}`;
}

export function findVirtualModelByAcceptedId(config: AppConfig, modelId: string): VirtualModel | undefined {
  return config.virtual_models.find((model) =>
    model.id === modelId
    || effectiveHostModelId(model) === modelId
    || virtualModelCatalogKey(model) === modelId
  );
}

export function nextHostModelId(occupied: Set<string>): string {
  for (let value = 400; value < 600; value += 1) {
    const candidate = `MODEL_PLACEHOLDER_M${value}`;
    if (!occupied.has(candidate)) {
      occupied.add(candidate);
      return candidate;
    }
  }
  throw new Error(t("models.hostModelSlotsExhausted"));
}

export function stripConfiguredModelSuffix(modelName: string, providerName: string): string {
  const knownSuffixes = [
    ` · ${providerName}`,
    ...["default", "off", "low", "medium", "high", "xhigh", "max", "auto"]
      .map((level) => ` ${level}(${providerName})`),
    `(${providerName})`,
  ];
  return knownSuffixes.reduce(
    (name, knownSuffix) => name.endsWith(knownSuffix) ? name.slice(0, -knownSuffix.length) : name,
    modelName,
  );
}

export function configuredModelDisplayName(
  modelName: string,
  providerName: string,
  reasoningLevel: ReasoningLevel | null,
  supportsReasoning: boolean,
): string {
  const baseName = stripConfiguredModelSuffix(modelName, providerName);
  if (!supportsReasoning) return `${baseName}(${providerName})`;

  const variant = reasoningLevel ?? "default";
  return `${baseName} ${variant.replace("_", "")}(${providerName})`;
}

export const emptyParameters = (): ParameterOverrides => ({
  temperature: null,
  max_tokens: null,
  top_p: null,
  top_k: null,
  extra_body: null,
});

export function protocolName(protocol: ProviderProtocol): string {
  return {
    openai_chat_completions: t("models.protocolOpenAI"),
    openai_responses: t("models.protocolResponses"),
    anthropic_messages: t("models.protocolAnthropic"),
    gemini_generate_content: t("models.protocolGemini"),
  }[protocol];
}
