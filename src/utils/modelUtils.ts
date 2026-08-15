import type { AppConfig, VirtualModel, ParameterOverrides, ProviderProtocol } from "../types/config";
import type { ReasoningLevel } from "../types/reasoning";
import { t } from "../i18n";

export const CUSTOM_HOST_MODEL_ID_PREFIX = "MODEL_PLACEHOLDER_M";
export const CUSTOM_HOST_MODEL_SLOT_PREFIX = "MODEL_PLACEHOLDER_";
export const CUSTOM_HOST_MODEL_ID_START = 400;
export const CUSTOM_HOST_MODEL_ID_END = 600;
const CUSTOM_HOST_MODEL_ID_SLOT_COUNT =
  CUSTOM_HOST_MODEL_ID_END - CUSTOM_HOST_MODEL_ID_START;
const MODEL_NAMESPACE_PREFIX = "models/";
const CUSTOM_MODEL_PREFIX = "custom-";
const CUSTOM_BYOK_MODEL_PREFIX = "custom-byok-";

export function effectiveHostModelId(model: VirtualModel): string {
  if (model.host_model_id) return model.host_model_id;
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(model.id)) {
    hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  }
  return `${CUSTOM_HOST_MODEL_ID_PREFIX}${CUSTOM_HOST_MODEL_ID_START + (hash % CUSTOM_HOST_MODEL_ID_SLOT_COUNT)}`;
}

function virtualModelCatalogKey(model: VirtualModel): string {
  const prefixedId = model.id.startsWith(CUSTOM_MODEL_PREFIX)
    ? model.id
    : `${CUSTOM_MODEL_PREFIX}${model.id}`;
  if (!prefixedId.includes("_")) return prefixedId;

  const hostModelId = effectiveHostModelId(model);
  const slot = hostModelId.replace(CUSTOM_HOST_MODEL_SLOT_PREFIX, "").toLowerCase();
  return `${CUSTOM_BYOK_MODEL_PREFIX}${slot}`;
}

export function findVirtualModelByAcceptedId(config: AppConfig, modelId: string): VirtualModel | undefined {
  const cleanId = modelId.startsWith(MODEL_NAMESPACE_PREFIX)
    ? modelId.slice(MODEL_NAMESPACE_PREFIX.length)
    : modelId;
  return config.virtual_models.find((model) =>
    model.id === cleanId
    || effectiveHostModelId(model) === cleanId
    || virtualModelCatalogKey(model) === cleanId
  );
}

export function nextHostModelId(occupied: Set<string>): string {
  for (let value = CUSTOM_HOST_MODEL_ID_START; value < CUSTOM_HOST_MODEL_ID_END; value += 1) {
    const candidate = `${CUSTOM_HOST_MODEL_ID_PREFIX}${value}`;
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
    ...["default", "off", "low", "medium", "high", "xhigh", "max", "adaptive", "auto"]
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
