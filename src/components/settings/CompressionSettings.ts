import { updateConfig } from "../../controllers/configController";
import { subscribeLanguage, t } from "../../i18n";
import { store } from "../../store/appStore";
import type {
  CheckpointExecutionPolicy,
  CompressionLimitsPolicy,
  ModelCheckpointOverride,
  OfficialModelSettings,
  UpstreamModel,
} from "../../types/config";
import { errorMessage } from "../../utils/errorUtils";
import { confirmHostAction } from "../ConfirmModal";
import { showNotice } from "../NoticeBar";
import {

  cloneCompressionSettings,
  cloneExecutionPolicy,
  compressionSettingsAreEqual,
  compressionSettingsAreValid,
  DEFAULT_COMPRESSION_SETTINGS,
  parseCompressionSettings,
  updateCompressionLimits,
} from "./compressionSettingsModel";
import {
  customModelCheckpointLimits,
  formatTokenLimit,
  isValidModelCheckpointOverride,
} from "../../features/providers/tokenLimits";

type CompressionPercentField =
  | "token_threshold_percent"
  | "max_token_limit_percent"
  | "max_output_tokens_percent";
type CompressionAbsoluteField = "token_threshold" | "max_token_limit" | "max_output_tokens";

interface CompressionControls {
  scope: "gemini" | "claude" | "custom_model";
  enabled: HTMLInputElement;
  mode: HTMLSelectElement;
  visualizer: HTMLElement | null;
  percentageSection: HTMLElement;
  absoluteSection: HTMLElement;
  percentageInputs: Record<CompressionPercentField, HTMLInputElement>;
  absoluteInputs: Record<CompressionAbsoluteField, HTMLInputElement>;
}

interface UpstreamCompressionDraft {
  id: string;
  upstream_model_id: string;
  checkpoint_override: ModelCheckpointOverride | null;
}

interface PolicyEditorControls {
  modelKey: string | null;
  enabled: HTMLInputElement;
  checkpointModel: HTMLSelectElement;
  strategy: HTMLInputElement;
  mode: HTMLSelectElement | null;
  thresholdPercent: HTMLInputElement | null;
  tokenThreshold: HTMLInputElement | null;
  maxTokenLimit: HTMLInputElement | null;
  maxOutputTokens: HTMLInputElement | null;
  maxOverheadRatio: HTMLInputElement;
  movingWindowSize: HTMLInputElement;
  useLastPlannerModel: HTMLInputElement;
  isSync: HTMLInputElement;
  maxUserRequests: HTMLInputElement;
  includeLastUserMessage: HTMLInputElement;
  includeConversationLog: HTMLInputElement;
  includeRunningTaskSnapshots: HTMLInputElement;
  includeSubagentSnapshots: HTMLInputElement;
  includeArtifactSnapshots: HTMLInputElement;
  maxRetries: HTMLInputElement;
  initialSleepDurationMs: HTMLInputElement;
  exponentialMultiplier: HTMLInputElement;
  includeErrorFeedback: HTMLInputElement;
}

const CHECKPOINT_EXECUTORS = [
  ["MODEL_PLACEHOLDER_M50", "settings.checkpointM50"],
  ["MODEL_PLACEHOLDER_M71", "settings.checkpointM71"],
  ["MODEL_PLACEHOLDER_M72", "settings.checkpointM72"],
] as const;

const COMPRESSION_PERCENT_FIELDS: readonly CompressionPercentField[] = [
  "token_threshold_percent",
  "max_token_limit_percent",
  "max_output_tokens_percent",
];

const COMPRESSION_ABSOLUTE_FIELDS: readonly CompressionAbsoluteField[] = [
  "token_threshold",
  "max_token_limit",
  "max_output_tokens",
];

const COMPRESSION_DOCUMENT_KEYS = ["official_model_settings", "upstream_models"] as const;
const UPSTREAM_DOCUMENT_KEYS = ["id", "upstream_model_id", "checkpoint_override"] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const expected = new Set(keys);
  const actual = Object.keys(value);
  return actual.length === expected.size && actual.every((key) => expected.has(key));
}

function createControls(scope: CompressionControls["scope"], prefix: string): CompressionControls | null {
  const enabled = document.querySelector<HTMLInputElement>(`#settings-${prefix}-enabled`);
  const mode = document.querySelector<HTMLSelectElement>(`#settings-${prefix}-mode`);
  const visualizer = document.querySelector<HTMLElement>(`#settings-${prefix}-visualizer`);
  const percentageSection = document.querySelector<HTMLElement>(`#settings-${prefix}-percentage-fields`);
  const absoluteSection = document.querySelector<HTMLElement>(`#settings-${prefix}-absolute-fields`);
  const percentageInputs = {
    token_threshold_percent: document.querySelector<HTMLInputElement>(`#settings-${prefix}-token-threshold-percent`),
    max_token_limit_percent: document.querySelector<HTMLInputElement>(`#settings-${prefix}-max-token-limit-percent`),
    max_output_tokens_percent: document.querySelector<HTMLInputElement>(`#settings-${prefix}-max-output-tokens-percent`),
  };
  const absoluteInputs = {
    token_threshold: document.querySelector<HTMLInputElement>(`#settings-${prefix}-token-threshold`),
    max_token_limit: document.querySelector<HTMLInputElement>(`#settings-${prefix}-max-token-limit`),
    max_output_tokens: document.querySelector<HTMLInputElement>(`#settings-${prefix}-max-output-tokens`),
  };
  if (!enabled || !mode || !percentageSection || !absoluteSection
    || Object.values(percentageInputs).some((input) => !input)
    || Object.values(absoluteInputs).some((input) => !input)) {
    return null;
  }
  return {
    scope,
    enabled,
    mode,
    visualizer,
    percentageSection,
    absoluteSection,
    percentageInputs: percentageInputs as Record<CompressionPercentField, HTMLInputElement>,
    absoluteInputs: absoluteInputs as Record<CompressionAbsoluteField, HTMLInputElement>,
  };
}

function cloneOverride(value: ModelCheckpointOverride | null): ModelCheckpointOverride | null {
  return value ? { ...value } : null;
}

function cloneUpstreamCompressionDrafts(models: UpstreamModel[]): UpstreamCompressionDraft[] {
  return models.map((model) => ({
    id: model.id,
    upstream_model_id: model.upstream_model_id,
    checkpoint_override: cloneOverride(model.checkpoint_override),
  }));
}

function compressionDraftsAreEqual(
  left: UpstreamCompressionDraft[],
  right: UpstreamCompressionDraft[],
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function applyUpstreamCompressionDrafts(
  models: UpstreamModel[],
  drafts: UpstreamCompressionDraft[],
): UpstreamModel[] {
  const byId = new Map(drafts.map((draft) => [draft.id, draft]));
  return models.map((model) => {
    const draft = byId.get(model.id);
    return draft
      ? { ...model, checkpoint_override: cloneOverride(draft.checkpoint_override) }
      : model;
  });
}

function checkpointModelLabel(modelId: string): string {
  const option = CHECKPOINT_EXECUTORS.find(([id]) => id === modelId);
  return option ? t(option[1]) : modelId;
}

function createLabel(text: string, input: HTMLElement): HTMLLabelElement {
  const label = document.createElement("label");
  label.className = "compression-policy-field";
  const caption = document.createElement("span");
  caption.textContent = text;
  label.append(caption, input);
  return label;
}

function createNumberInput(value: number | string, min = 0, max?: number): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "number";
  input.min = String(min);
  if (max !== undefined) input.max = String(max);
  input.step = "1";
  input.inputMode = "numeric";
  input.value = String(value);
  return input;
}

function createTextInput(value: string): HTMLInputElement {
  const input = document.createElement("input");
  input.type = "text";
  input.value = value;
  return input;
}

function createCheckboxLabel(text: string, value: boolean): {
  label: HTMLLabelElement;
  input: HTMLInputElement;
} {
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = value;
  const label = document.createElement("label");
  label.className = "compression-policy-checkbox";
  label.append(input, document.createTextNode(text));
  return { label, input };
}

function createPolicyEditorControls(
  container: HTMLElement,
  modelKey: string | null,
  modelName: string,
  policy: CheckpointExecutionPolicy,
  override: ModelCheckpointOverride | null,
  operationInProgress: boolean,
  hasExplicitPolicy: boolean,
  onChange: () => void,
  onResetModel: () => void,
): PolicyEditorControls {
  container.replaceChildren();

  const heading = document.createElement("div");
  heading.className = "compression-policy-editor-heading";
  const title = document.createElement("strong");
  title.textContent = modelKey === null ? t("settings.globalCheckpointPolicy") : modelName;
  const key = document.createElement("code");
  key.textContent = modelKey ?? t("settings.globalCheckpointPolicyKey");
  heading.append(title, key);
  if (modelKey !== null && !hasExplicitPolicy) {
    const inherited = document.createElement("span");
    inherited.className = "compression-policy-inherited";
    inherited.textContent = t("settings.checkpointPolicyInherited");
    heading.append(inherited);
  }
  container.append(heading);

  const primaryGrid = document.createElement("div");
  primaryGrid.className = "compression-policy-grid";
  const enabled = createCheckboxLabel(t("settings.checkpointEnabled"), policy.enabled);
  enabled.label.classList.add("compression-policy-checkbox-primary");
  primaryGrid.append(enabled.label);

  const checkpointModel = document.createElement("select");
  checkpointModel.className = "custom-select";
  for (const [id, labelKey] of CHECKPOINT_EXECUTORS) {
    const option = document.createElement("option");
    option.value = id;
    option.textContent = t(labelKey);
    checkpointModel.append(option);
  }

  checkpointModel.value = policy.checkpoint_model;
  primaryGrid.append(createLabel(t("settings.checkpointExecutor"), checkpointModel));

  const strategy = createTextInput(policy.strategy);
  primaryGrid.append(createLabel(t("settings.checkpointStrategy"), strategy));
  container.append(primaryGrid);

  let mode: HTMLSelectElement | null = null;
  let thresholdPercent: HTMLInputElement | null = null;
  let tokenThreshold: HTMLInputElement | null = null;
  let maxTokenLimit: HTMLInputElement | null = null;
  let maxOutputTokens: HTMLInputElement | null = null;

  if (modelKey !== null) {
    const limitSection = document.createElement("div");
    limitSection.className = "compression-policy-limit-section";
    const limitHeading = document.createElement("h4");
    limitHeading.textContent = t("settings.checkpointLimits");
    limitSection.append(limitHeading);

    mode = document.createElement("select");
    mode.className = "custom-select";
    for (const [value, labelKey] of [
      ["global", "settings.checkpointFollowGlobal"],
      ["percentage", "settings.checkpointPercentage"],
      ["custom", "settings.checkpointCustom"],
    ] as const) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = t(labelKey);
      mode.append(option);
    }
    mode.value = override?.kind ?? "global";
    limitSection.append(createLabel(t("settings.checkpointLimitMode"), mode));

    const percentageGrid = document.createElement("div");
    percentageGrid.className = "compression-policy-grid compression-policy-limit-fields";
    thresholdPercent = createNumberInput(
      override?.kind === "percentage" ? override.threshold_percent : 61,
      1,
      100,
    );
    percentageGrid.append(createLabel(t("settings.checkpointThresholdPercentage"), thresholdPercent));

    const customGrid = document.createElement("div");
    customGrid.className = "compression-policy-grid compression-policy-limit-fields";
    const customOverride = override?.kind === "custom" ? override : null;
    tokenThreshold = createNumberInput(customOverride?.token_threshold ?? 1, 1);
    maxTokenLimit = createNumberInput(customOverride?.max_token_limit ?? 2, 1);
    maxOutputTokens = createNumberInput(customOverride?.max_output_tokens ?? 1, 1);
    customGrid.append(
      createLabel(t("models.checkpointThreshold"), tokenThreshold),
      createLabel(t("models.checkpointHardLimit"), maxTokenLimit),
      createLabel(t("models.checkpointOutputReserve"), maxOutputTokens),
    );
    limitSection.append(percentageGrid, customGrid);

    const updateLimitVisibility = () => {
      percentageGrid.hidden = mode?.value !== "percentage";
      customGrid.hidden = mode?.value !== "custom";
    };
    mode.addEventListener("change", () => {
      updateLimitVisibility();
      onChange();
    });
    updateLimitVisibility();
    container.append(limitSection);
  }

  const advanced = document.createElement("details");
  advanced.className = "compression-policy-advanced";
  const advancedSummary = document.createElement("summary");
  advancedSummary.textContent = t("settings.checkpointAdvanced");
  advanced.append(advancedSummary);

  const advancedGrid = document.createElement("div");
  advancedGrid.className = "compression-policy-grid";
  const maxOverheadRatio = createTextInput(policy.max_overhead_ratio);
  const movingWindowSize = createTextInput(policy.moving_window_size);
  const useLastPlannerModel = createCheckboxLabel(
    t("settings.checkpointUseLastPlannerModel"),
    policy.use_last_planner_model,
  );
  const isSync = createCheckboxLabel(t("settings.checkpointIsSync"), policy.is_sync);
  const maxUserRequests = createNumberInput(policy.max_user_requests);
  advancedGrid.append(
    createLabel(t("settings.checkpointMaxOverheadRatio"), maxOverheadRatio),
    createLabel(t("settings.checkpointMovingWindowSize"), movingWindowSize),
    useLastPlannerModel.label,
    isSync.label,
    createLabel(t("settings.checkpointMaxUserRequests"), maxUserRequests),
  );
  advanced.append(advancedGrid);

  const snapshotGrid = document.createElement("div");
  snapshotGrid.className = "compression-policy-checkbox-grid";
  const includeLastUserMessage = createCheckboxLabel(
    t("settings.checkpointIncludeLastUserMessage"),
    policy.include_last_user_message,
  );
  const includeConversationLog = createCheckboxLabel(
    t("settings.checkpointIncludeConversationLog"),
    policy.include_conversation_log,
  );
  const includeRunningTaskSnapshots = createCheckboxLabel(
    t("settings.checkpointIncludeRunningTaskSnapshots"),
    policy.include_running_task_snapshots,
  );
  const includeSubagentSnapshots = createCheckboxLabel(
    t("settings.checkpointIncludeSubagentSnapshots"),
    policy.include_subagent_snapshots,
  );
  const includeArtifactSnapshots = createCheckboxLabel(
    t("settings.checkpointIncludeArtifactSnapshots"),
    policy.include_artifact_snapshots,
  );
  snapshotGrid.append(
    includeLastUserMessage.label,
    includeConversationLog.label,
    includeRunningTaskSnapshots.label,
    includeSubagentSnapshots.label,
    includeArtifactSnapshots.label,
  );
  advanced.append(snapshotGrid);

  const retryHeading = document.createElement("h4");
  retryHeading.textContent = t("settings.checkpointRetryConfig");
  advanced.append(retryHeading);
  const retryGrid = document.createElement("div");
  retryGrid.className = "compression-policy-grid";
  const maxRetries = createNumberInput(policy.retry_config.max_retries);
  const initialSleepDurationMs = createNumberInput(policy.retry_config.initial_sleep_duration_ms);
  const exponentialMultiplier = createNumberInput(policy.retry_config.exponential_multiplier);
  const includeErrorFeedback = createCheckboxLabel(
    t("settings.checkpointIncludeErrorFeedback"),
    policy.retry_config.include_error_feedback,
  );
  retryGrid.append(
    createLabel(t("settings.checkpointMaxRetries"), maxRetries),
    createLabel(t("settings.checkpointInitialSleep"), initialSleepDurationMs),
    createLabel(t("settings.checkpointExponentialMultiplier"), exponentialMultiplier),
    includeErrorFeedback.label,
  );
  advanced.append(retryGrid);
  container.append(advanced);

  if (modelKey !== null) {
    const reset = document.createElement("button");
    reset.type = "button";
    reset.className = "secondary compact-button compression-policy-reset";
    reset.textContent = t("settings.checkpointUseGlobal");
    reset.disabled = !hasExplicitPolicy || operationInProgress;
    reset.addEventListener("click", onResetModel);
    container.append(reset);
  }

  const controls: PolicyEditorControls = {
    modelKey,
    enabled: enabled.input,
    checkpointModel,
    strategy,
    mode,
    thresholdPercent,
    tokenThreshold,
    maxTokenLimit,
    maxOutputTokens,
    maxOverheadRatio,
    movingWindowSize,
    useLastPlannerModel: useLastPlannerModel.input,
    isSync: isSync.input,
    maxUserRequests,
    includeLastUserMessage: includeLastUserMessage.input,
    includeConversationLog: includeConversationLog.input,
    includeRunningTaskSnapshots: includeRunningTaskSnapshots.input,
    includeSubagentSnapshots: includeSubagentSnapshots.input,
    includeArtifactSnapshots: includeArtifactSnapshots.input,
    maxRetries,
    initialSleepDurationMs,
    exponentialMultiplier,
    includeErrorFeedback: includeErrorFeedback.input,
  };

  const editorInputs: Array<HTMLInputElement | HTMLSelectElement> = [
    enabled.input,
    checkpointModel,
    strategy,
    ...(mode ? [mode] : []),
    ...(thresholdPercent ? [thresholdPercent] : []),
    ...(tokenThreshold ? [tokenThreshold] : []),
    ...(maxTokenLimit ? [maxTokenLimit] : []),
    ...(maxOutputTokens ? [maxOutputTokens] : []),
    maxOverheadRatio,
    movingWindowSize,
    useLastPlannerModel.input,
    isSync.input,
    maxUserRequests,
    includeLastUserMessage.input,
    includeConversationLog.input,
    includeRunningTaskSnapshots.input,
    includeSubagentSnapshots.input,
    includeArtifactSnapshots.input,
    maxRetries,
    initialSleepDurationMs,
    exponentialMultiplier,
    includeErrorFeedback.input,
  ];
  enabled.input.checked = true;
  enabled.input.disabled = true;
  for (const input of editorInputs) {
    input.addEventListener("input", onChange);
    input.addEventListener("change", onChange);
  }
  return controls;
}

function numberOr(input: HTMLInputElement, fallback: number): number {
  const value = Number(input.value);
  return Number.isInteger(value) && value >= 0 ? value : fallback;
}

function policyFromEditor(controls: PolicyEditorControls): CheckpointExecutionPolicy {
  return {
    enabled: true,
    checkpoint_model: controls.checkpointModel.value,
    strategy: controls.strategy.value.trim(),
    max_overhead_ratio: controls.maxOverheadRatio.value.trim(),
    moving_window_size: controls.movingWindowSize.value.trim(),
    use_last_planner_model: controls.useLastPlannerModel.checked,
    is_sync: controls.isSync.checked,
    max_user_requests: numberOr(controls.maxUserRequests, 0),
    include_last_user_message: controls.includeLastUserMessage.checked,
    include_conversation_log: controls.includeConversationLog.checked,
    include_running_task_snapshots: controls.includeRunningTaskSnapshots.checked,
    include_subagent_snapshots: controls.includeSubagentSnapshots.checked,
    include_artifact_snapshots: controls.includeArtifactSnapshots.checked,
    retry_config: {
      max_retries: numberOr(controls.maxRetries, 0),
      initial_sleep_duration_ms: numberOr(controls.initialSleepDurationMs, 0),
      exponential_multiplier: numberOr(controls.exponentialMultiplier, 0),
      include_error_feedback: controls.includeErrorFeedback.checked,
    },
  };
}

function overrideFromEditor(controls: PolicyEditorControls): ModelCheckpointOverride | null {
  if (!controls.mode || controls.mode.value === "global") return null;
  if (controls.mode.value === "percentage") {
    return {
      kind: "percentage",
      threshold_percent: numberOr(controls.thresholdPercent ?? document.createElement("input"), 0),
    };
  }
  return {
    kind: "custom",
    token_threshold: numberOr(controls.tokenThreshold ?? document.createElement("input"), 0),
    max_token_limit: numberOr(controls.maxTokenLimit ?? document.createElement("input"), 0),
    max_output_tokens: numberOr(controls.maxOutputTokens ?? document.createElement("input"), 0),
  };
}

function parseModelCheckpointOverride(value: unknown): ModelCheckpointOverride | null | undefined {
  if (value === null) return null;
  if (!isRecord(value) || typeof value.kind !== "string") return undefined;
  if (value.kind === "percentage") {
    if (!hasExactKeys(value, ["kind", "threshold_percent"])
      || typeof value.threshold_percent !== "number"
      || !Number.isInteger(value.threshold_percent)
      || value.threshold_percent < 1
      || value.threshold_percent > 100) {
      return undefined;
    }
    return { kind: "percentage", threshold_percent: value.threshold_percent };
  }
  if (value.kind === "custom") {
    if (!hasExactKeys(value, ["kind", "token_threshold", "max_token_limit", "max_output_tokens"])
      || ![value.token_threshold, value.max_token_limit, value.max_output_tokens].every(
        (item) => typeof item === "number" && Number.isInteger(item) && item > 0,
      )) {
      return undefined;
    }
    const override = {
      kind: "custom" as const,
      token_threshold: value.token_threshold as number,
      max_token_limit: value.max_token_limit as number,
      max_output_tokens: value.max_output_tokens as number,
    };
    return isValidModelCheckpointOverride(override) ? override : undefined;
  }
  return undefined;
}

function createCompressionDocument(
  settings: OfficialModelSettings,
  drafts: UpstreamCompressionDraft[],
): Record<string, unknown> {
  return {
    official_model_settings: cloneCompressionSettings(settings),
    upstream_models: drafts.map((draft) => ({
      id: draft.id,
      upstream_model_id: draft.upstream_model_id,
      checkpoint_override: cloneOverride(draft.checkpoint_override),
    })),
  };
}

function parseCompressionDocument(
  value: unknown,
  currentDrafts: UpstreamCompressionDraft[],
): { settings: OfficialModelSettings; drafts: UpstreamCompressionDraft[] } | null {
  if (!isRecord(value) || !hasExactKeys(value, COMPRESSION_DOCUMENT_KEYS)) return null;
  const settings = parseCompressionSettings(value.official_model_settings);
  if (!settings || !Array.isArray(value.upstream_models)) return null;
  const currentModelKeys = new Set(currentDrafts.map((draft) => draft.upstream_model_id));
  if (Object.keys(settings.model_checkpoint_policies).some((modelId) => !currentModelKeys.has(modelId))) {
    return null;
  }

  const currentById = new Map(currentDrafts.map((draft) => [draft.id, draft]));
  const drafts: UpstreamCompressionDraft[] = [];
  const seen = new Set<string>();
  for (const item of value.upstream_models) {
    if (!isRecord(item) || !hasExactKeys(item, UPSTREAM_DOCUMENT_KEYS)
      || typeof item.id !== "string"
      || typeof item.upstream_model_id !== "string") {
      return null;
    }
    const current = currentById.get(item.id);
    if (!current || current.upstream_model_id !== item.upstream_model_id || seen.has(item.id)) return null;
    const override = parseModelCheckpointOverride(item.checkpoint_override);
    if (override === undefined) return null;
    drafts.push({
      id: current.id,
      upstream_model_id: current.upstream_model_id,
      checkpoint_override: cloneOverride(override),
    });
    seen.add(item.id);
  }
  if (drafts.length !== currentDrafts.length) return null;
  return { settings, drafts };
}

function getScopeCapacity(scope: CompressionControls["scope"]): number {
  switch (scope) {
    case "gemini":
      return 1_048_576;
    case "claude":
      return 200_000;
    case "custom_model":
      return 128_000;
  }
}

function renderControlVisualizer(control: CompressionControls, settings: CompressionLimitsPolicy): void {
  if (!control.visualizer) return;
  const capacity = getScopeCapacity(control.scope);
  let triggerPct = 0;
  let hardLimitPct = 0;
  let reservePct = 0;

  if (settings.mode === "percentage") {
    triggerPct = Math.min(100, Math.max(0, settings.token_threshold_percent));
    hardLimitPct = Math.min(100, Math.max(0, settings.max_token_limit_percent));
    reservePct = Math.min(100, Math.max(0, settings.max_output_tokens_percent));
  } else {
    triggerPct = Math.min(100, Math.round((settings.token_threshold / capacity) * 100));
    hardLimitPct = Math.min(100, Math.round((settings.max_token_limit / capacity) * 100));
    reservePct = Math.min(100, Math.round((settings.max_output_tokens / capacity) * 100));
  }

  const activeWidth = Math.min(100, triggerPct);
  const bufferWidth = Math.max(0, Math.min(100 - activeWidth, hardLimitPct - triggerPct));
  const reserveWidth = Math.max(0, Math.min(100 - activeWidth - bufferWidth, 100 - hardLimitPct));

  control.visualizer.replaceChildren();

  const bar = document.createElement("div");
  bar.className = "context-stack-bar";

  const segActive = document.createElement("div");
  segActive.className = "bar-segment segment-active";
  segActive.style.width = `${activeWidth}%`;

  const segBuffer = document.createElement("div");
  segBuffer.className = "bar-segment segment-buffer";
  segBuffer.style.width = `${bufferWidth}%`;

  const segReserve = document.createElement("div");
  segReserve.className = "bar-segment segment-reserve";
  segReserve.style.width = `${reserveWidth}%`;

  bar.append(segActive, segBuffer, segReserve);

  const legend = document.createElement("div");
  legend.className = "context-legend";

  const item1 = document.createElement("span");
  item1.className = "context-legend-item";
  const dot1 = document.createElement("span");
  dot1.className = "context-legend-dot dot-active";
  item1.append(dot1, document.createTextNode(`${t("settings.contextVisualizerTrigger")}: ${triggerPct}%`));

  const item2 = document.createElement("span");
  item2.className = "context-legend-item";
  const dot2 = document.createElement("span");
  dot2.className = "context-legend-dot dot-buffer";
  item2.append(dot2, document.createTextNode(`${t("settings.contextVisualizerHardLimit")}: ${hardLimitPct}%`));

  const item3 = document.createElement("span");
  item3.className = "context-legend-item";
  const dot3 = document.createElement("span");
  dot3.className = "context-legend-dot dot-reserve";
  item3.append(dot3, document.createTextNode(`${t("settings.contextVisualizerReserve")}: ${reservePct}%`));

  legend.append(item1, item2, item3);
  control.visualizer.append(bar, legend);
}

class CompressionSettingsController {
  private savedSettings = cloneCompressionSettings(store.config.official_model_settings);
  private draftSettings = cloneCompressionSettings(this.savedSettings);
  private savedDrafts = cloneUpstreamCompressionDrafts(store.config.upstream_models);
  private draftDrafts = cloneUpstreamCompressionDrafts(store.config.upstream_models);
  private operationInProgress = false;
  private selectedModelKey: string | null = null;
  private policyEditor: PolicyEditorControls | null = null;
  private jsonEditing = false;

  constructor(
    private readonly controls: CompressionControls[],
    private readonly resetButton: HTMLButtonElement,
    private readonly saveButton: HTMLButtonElement,
    private readonly source: HTMLElement,
    private readonly modelList: HTMLElement,
    private readonly policyEditorContainer: HTMLElement,
    private readonly globalPolicyButton: HTMLButtonElement,
    private readonly policySaveButton: HTMLButtonElement,
    private readonly jsonEditor: HTMLTextAreaElement,
    private readonly jsonEditButton: HTMLButtonElement,
    private readonly jsonFormatButton: HTMLButtonElement,
    private readonly jsonSaveButton: HTMLButtonElement,
    private readonly jsonStatus: HTMLElement,
  ) {}

  start(): void {
    for (const control of this.controls) {
      control.enabled.addEventListener("change", () => this.changeLimits(control));
      control.mode.addEventListener("change", () => this.changeLimits(control, true));
      for (const input of Object.values(control.percentageInputs)) {
        input.addEventListener("input", () => this.changeLimits(control));
      }
      for (const input of Object.values(control.absoluteInputs)) {
        input.addEventListener("input", () => this.changeLimits(control));
      }
    }
    this.resetButton.addEventListener("click", () => void this.reset());
    this.saveButton.addEventListener("click", () => void this.save());
    this.globalPolicyButton.addEventListener("click", () => {
      this.syncEditorToDraft();
      this.selectedModelKey = null;
      this.renderPolicyEditor(true);
    });
    this.policySaveButton.addEventListener("click", () => void this.save());
    this.jsonEditButton.addEventListener("click", () => this.toggleJsonEditing());
    this.jsonFormatButton.addEventListener("click", () => this.formatJson());
    this.jsonSaveButton.addEventListener("click", () => void this.saveJson());
    store.subscribeConfig(() => this.syncFromStore());
    subscribeLanguage(() => {
      this.syncEditorToDraft();
      this.render(true);
    });
    this.render(true);
  }

  private hasDraftChanges(): boolean {
    return !compressionSettingsAreEqual(this.savedSettings, this.draftSettings)
      || !compressionDraftsAreEqual(this.savedDrafts, this.draftDrafts);
  }

  private render(writeValues: boolean): void {
    const configAvailable = store.configLoaded;
    for (const control of this.controls) {
      const settings = this.draftSettings[control.scope];
      control.enabled.disabled = !configAvailable
        || this.operationInProgress
        || control.scope === "custom_model";
      control.mode.disabled = !configAvailable || this.operationInProgress;
      control.percentageSection.hidden = settings.mode !== "percentage";
      control.absoluteSection.hidden = settings.mode !== "absolute";
      for (const field of COMPRESSION_PERCENT_FIELDS) {
        const input = control.percentageInputs[field];
        input.disabled = !configAvailable || this.operationInProgress || !settings.enabled;
        if (writeValues) input.value = String(settings[field]);
      }
      for (const field of COMPRESSION_ABSOLUTE_FIELDS) {
        const input = control.absoluteInputs[field];
        input.disabled = !configAvailable || this.operationInProgress || !settings.enabled;
        if (writeValues) input.value = String(settings[field]);
      }
      if (writeValues) {
        control.enabled.checked = control.scope === "custom_model" ? true : settings.enabled;
        control.mode.value = settings.mode;
      }
      renderControlVisualizer(control, settings);
    }

    this.source.textContent = t("settings.compressionStrategyStatus");
    this.renderCustomModels();
    if (writeValues || !this.policyEditor) this.renderPolicyEditor(true);
    else this.setPolicyEditorDisabled();
    this.renderJsonEditor(writeValues);

    const changed = this.hasDraftChanges();
    const valid = compressionSettingsAreValid(this.draftSettings) && this.draftsAreValid();
    this.resetButton.disabled = this.operationInProgress || !configAvailable || !changed;
    this.saveButton.disabled = this.operationInProgress || !configAvailable || !changed || !valid;
    this.policySaveButton.disabled = this.saveButton.disabled;
  }

  private changeLimits(control: CompressionControls, isModeChange = false): void {
    const parseField = (input: HTMLInputElement): number => {
      const value = Number(input.value);
      return Number.isInteger(value) && value >= 0 ? value : 0;
    };
    const capacity = getScopeCapacity(control.scope);
    const newMode = control.mode.value === "absolute" ? "absolute" : "percentage";

    if (isModeChange) {
      if (newMode === "absolute") {
        const thresholdPct = parseField(control.percentageInputs.token_threshold_percent);
        const limitPct = parseField(control.percentageInputs.max_token_limit_percent);
        const reservePct = parseField(control.percentageInputs.max_output_tokens_percent);
        control.absoluteInputs.token_threshold.value = String(Math.round((capacity * thresholdPct) / 100));
        control.absoluteInputs.max_token_limit.value = String(Math.round((capacity * limitPct) / 100));
        control.absoluteInputs.max_output_tokens.value = String(Math.round((capacity * reservePct) / 100));
      } else {
        const thresholdToken = parseField(control.absoluteInputs.token_threshold);
        const limitToken = parseField(control.absoluteInputs.max_token_limit);
        const reserveToken = parseField(control.absoluteInputs.max_output_tokens);
        if (thresholdToken > 0 && limitToken > 0) {
          control.percentageInputs.token_threshold_percent.value = String(Math.min(100, Math.round((thresholdToken / capacity) * 100)));
          control.percentageInputs.max_token_limit_percent.value = String(Math.min(100, Math.round((limitToken / capacity) * 100)));
          control.percentageInputs.max_output_tokens_percent.value = String(Math.min(100, Math.round((reserveToken / capacity) * 100)));
        }
      }
    }

    const patch: Partial<CompressionLimitsPolicy> = {
      enabled: control.scope === "custom_model" ? true : control.enabled.checked,
      mode: newMode,
    };
    for (const field of COMPRESSION_PERCENT_FIELDS) patch[field] = parseField(control.percentageInputs[field]);
    for (const field of COMPRESSION_ABSOLUTE_FIELDS) patch[field] = parseField(control.absoluteInputs[field]);
    this.draftSettings = updateCompressionLimits(this.draftSettings, control.scope, patch);
    this.render(false);
  }

  private renderCustomModels(): void {
    this.modelList.replaceChildren();
    const models = store.config.upstream_models;
    if (models.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty-state compact-empty";
      empty.textContent = t("settings.noCustomModels");
      this.modelList.append(empty);
      return;
    }

    for (const model of models) {
      const key = model.upstream_model_id;
      const policy = this.draftSettings.model_checkpoint_policies[key]
        ?? this.draftSettings.custom_model_checkpoint;
      const draft = this.draftDrafts.find((item) => item.id === model.id);
      const limits = customModelCheckpointLimits(
        this.draftSettings,
        model.token_limits,
        draft?.checkpoint_override ?? null,
      );
      const row = document.createElement("article");
      row.className = "compression-custom-model-row";
      const copy = document.createElement("div");
      copy.className = "compression-custom-model-copy";
      const name = document.createElement("strong");
      name.textContent = model.display_name;
      const id = document.createElement("code");
      id.textContent = key;
      const facts = document.createElement("span");
      facts.className = "compression-custom-model-facts";
      const status = policy.enabled ? t("settings.checkpointActive") : t("settings.checkpointDisabled");
      const limitText = limits
        ? t("settings.checkpointLimitsSummary", {
            threshold: formatTokenLimit(limits.threshold),
            hard: formatTokenLimit(limits.max_token_limit),
            output: formatTokenLimit(limits.max_output_tokens),
          })
        : t("settings.checkpointLimitsUnavailable");
      facts.textContent = `${status} · ${checkpointModelLabel(policy.checkpoint_model)} · ${limitText}`;
      copy.append(name, id, facts);

      const edit = document.createElement("button");
      edit.type = "button";
      edit.className = "secondary compact-button";
      edit.textContent = t("settings.editCheckpointPolicy");
      edit.disabled = this.operationInProgress;
      edit.addEventListener("click", () => {
        this.syncEditorToDraft();
        this.selectedModelKey = key;
        this.renderPolicyEditor(true);
      });
      row.append(copy, edit);
      this.modelList.append(row);
    }
  }

  private renderPolicyEditor(force: boolean): void {
    if (!force && this.policyEditor) return;
    const selectedModel = this.selectedModelKey
      ? store.config.upstream_models.find((item) => item.upstream_model_id === this.selectedModelKey)
      : undefined;
    if (this.selectedModelKey && !selectedModel) this.selectedModelKey = null;
    const modelKey = this.selectedModelKey;
    const currentModel = modelKey
      ? store.config.upstream_models.find((item) => item.upstream_model_id === modelKey)
      : undefined;
    const policy = modelKey
      ? this.draftSettings.model_checkpoint_policies[modelKey]
        ?? this.draftSettings.custom_model_checkpoint
      : this.draftSettings.custom_model_checkpoint;
    const draft = currentModel
      ? this.draftDrafts.find((item) => item.id === currentModel.id)
      : undefined;
    const hasExplicitPolicy = modelKey !== null
      && this.draftSettings.model_checkpoint_policies[modelKey] !== undefined;
    this.policyEditor = createPolicyEditorControls(
      this.policyEditorContainer,
      modelKey,
      currentModel?.display_name ?? t("settings.globalCheckpointPolicy"),
      cloneExecutionPolicy(policy),
      draft?.checkpoint_override ?? null,
      this.operationInProgress,
      hasExplicitPolicy,
      () => this.syncEditorToDraft(),
      () => this.resetModelPolicy(),
    );
    this.globalPolicyButton.disabled = this.operationInProgress;
  }

  private setPolicyEditorDisabled(): void {
    if (!this.policyEditor) return;
    for (const input of Object.values(this.policyEditor)) {
      if (input instanceof HTMLInputElement || input instanceof HTMLSelectElement) {
        input.disabled = input === this.policyEditor.enabled || this.operationInProgress;
      }
    }
  }

  private syncEditorToDraft(): void {
    if (!this.policyEditor) return;
    const policy = policyFromEditor(this.policyEditor);
    const modelKey = this.policyEditor.modelKey;
    if (modelKey === null) {
      this.draftSettings.custom_model_checkpoint = policy;
      this.render(false);
      return;
    }
    this.draftSettings.model_checkpoint_policies[modelKey] = policy;
    const model = store.config.upstream_models.find((item) => item.upstream_model_id === modelKey);
    if (model) {
      const draft = this.draftDrafts.find((item) => item.id === model.id);
      if (draft) draft.checkpoint_override = overrideFromEditor(this.policyEditor);
    }
    this.render(false);
  }

  private resetModelPolicy(): void {
    const modelKey = this.selectedModelKey;
    if (!modelKey) return;
    this.syncEditorToDraft();
    delete this.draftSettings.model_checkpoint_policies[modelKey];
    const model = store.config.upstream_models.find((item) => item.upstream_model_id === modelKey);
    const draft = model ? this.draftDrafts.find((item) => item.id === model.id) : undefined;
    if (draft) draft.checkpoint_override = null;
    this.renderPolicyEditor(true);
    this.render(false);
  }

  private draftsAreValid(): boolean {
    return this.draftDrafts.every((draft) => isValidModelCheckpointOverride(draft.checkpoint_override));
  }

  private configIsAvailable(): boolean {
    if (store.configLoaded) return true;
    showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
    return false;
  }

  private async reset(): Promise<void> {
    if (this.operationInProgress || !this.configIsAvailable()) return;
    this.operationInProgress = true;
    this.render(false);
    try {
      const confirmed = await confirmHostAction(
        t("settings.compressionResetConfirm"),
        t("settings.compressionResetConfirmTitle"),
        t("settings.compressionResetConfirmOk"),
        t("models.cancel"),
      );
      if (!confirmed) return;
      this.draftSettings = cloneCompressionSettings(DEFAULT_COMPRESSION_SETTINGS);
      this.draftDrafts = this.draftDrafts.map((draft) => ({ ...draft, checkpoint_override: null }));
      this.selectedModelKey = null;
      showNotice(t("settings.compressionResetNotice"), "success");
    } catch (error) {
      showNotice(errorMessage(error), "error");
    } finally {
      this.operationInProgress = false;
      this.render(true);
    }
  }

  private async save(): Promise<void> {
    this.syncEditorToDraft();
    if (this.operationInProgress || !this.configIsAvailable()) return;
    if (!compressionSettingsAreValid(this.draftSettings) || !this.draftsAreValid()) {
      showNotice(t("settings.compressionInvalid"), "error");
      return;
    }
    this.operationInProgress = true;
    this.render(false);
    try {
      const confirmed = await confirmHostAction(
        t("settings.compressionSaveConfirm"),
        t("settings.compressionSaveConfirmTitle"),
        t("settings.compressionSaveConfirmOk"),
        t("models.cancel"),
      );
      if (!confirmed) return;
      const draftSettings = cloneCompressionSettings(this.draftSettings);
      const draftDrafts = this.draftDrafts.map((draft) => ({
        ...draft,
        checkpoint_override: cloneOverride(draft.checkpoint_override),
      }));
      const savedConfig = await updateConfig((current) => ({
        ...current,
        official_model_settings: draftSettings,
        upstream_models: applyUpstreamCompressionDrafts(current.upstream_models, draftDrafts),
      }));
      this.savedSettings = cloneCompressionSettings(savedConfig.official_model_settings);
      this.draftSettings = cloneCompressionSettings(this.savedSettings);
      this.savedDrafts = cloneUpstreamCompressionDrafts(savedConfig.upstream_models);
      this.draftDrafts = cloneUpstreamCompressionDrafts(savedConfig.upstream_models);
      showNotice(t("settings.compressionSaved"), "success");
    } catch (error) {
      showNotice(t("settings.compressionSaveFailed", { message: errorMessage(error) }), "error");
    } finally {
      this.operationInProgress = false;
      this.render(true);
    }
  }

  private renderJsonEditor(writeValues: boolean): void {
    if (writeValues && !this.jsonEditing) {
      this.jsonEditor.value = JSON.stringify(
        createCompressionDocument(this.draftSettings, this.draftDrafts),
        null,
        2,
      );
    }
    this.jsonEditor.readOnly = !this.jsonEditing || this.operationInProgress;
    this.jsonEditButton.textContent = this.jsonEditing
      ? t("settings.compressionJsonView")
      : t("settings.compressionJsonEdit");
    this.jsonEditButton.disabled = this.operationInProgress;
    this.jsonFormatButton.disabled = !this.jsonEditing || this.operationInProgress;
    this.jsonSaveButton.disabled = !this.jsonEditing || this.operationInProgress || !store.configLoaded;
  }

  private toggleJsonEditing(): void {
    if (this.jsonEditing) {
      this.jsonEditing = false;
      this.renderJsonEditor(true);
      this.jsonStatus.textContent = t("settings.compressionJsonViewStatus");
      return;
    }
    this.jsonEditing = true;
    this.jsonStatus.textContent = t("settings.compressionJsonEditStatus");
    this.renderJsonEditor(false);
  }

  private formatJson(): void {
    try {
      this.jsonEditor.value = JSON.stringify(JSON.parse(this.jsonEditor.value), null, 2);
      this.jsonStatus.textContent = t("settings.compressionJsonFormatted");
    } catch (error) {
      this.jsonStatus.textContent = t("settings.compressionJsonInvalid", { message: errorMessage(error) });
    }
  }

  private async saveJson(): Promise<void> {
    if (!this.jsonEditing || this.operationInProgress || !this.configIsAvailable()) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(this.jsonEditor.value);
    } catch (error) {
      this.jsonStatus.textContent = t("settings.compressionJsonInvalid", { message: errorMessage(error) });
      return;
    }
    const document = parseCompressionDocument(parsed, this.draftDrafts);
    if (!document) {
      this.jsonStatus.textContent = t("settings.compressionJsonValidationFailed");
      return;
    }
    this.operationInProgress = true;
    this.render(false);
    try {
      const savedConfig = await updateConfig((current) => ({
        ...current,
        official_model_settings: document.settings,
        upstream_models: applyUpstreamCompressionDrafts(current.upstream_models, document.drafts),
      }));
      this.savedSettings = cloneCompressionSettings(savedConfig.official_model_settings);
      this.draftSettings = cloneCompressionSettings(this.savedSettings);
      this.savedDrafts = cloneUpstreamCompressionDrafts(savedConfig.upstream_models);
      this.draftDrafts = cloneUpstreamCompressionDrafts(savedConfig.upstream_models);
      this.jsonEditing = false;
      this.jsonStatus.textContent = t("settings.compressionJsonSaved");
      showNotice(t("settings.compressionSaved"), "success");
    } catch (error) {
      this.jsonStatus.textContent = t("settings.compressionJsonSaveFailed", { message: errorMessage(error) });
      showNotice(errorMessage(error), "error");
    } finally {
      this.operationInProgress = false;
      this.render(true);
    }
  }

  private syncFromStore(): void {
    const incomingSettings = cloneCompressionSettings(store.config.official_model_settings);
    const incomingDrafts = cloneUpstreamCompressionDrafts(store.config.upstream_models);
    if (compressionSettingsAreEqual(this.savedSettings, incomingSettings)
      && compressionDraftsAreEqual(this.savedDrafts, incomingDrafts)) {
      this.render(false);
      return;
    }
    if (this.hasDraftChanges()) return;
    this.savedSettings = incomingSettings;
    this.draftSettings = cloneCompressionSettings(incomingSettings);
    this.savedDrafts = incomingDrafts;
    this.draftDrafts = cloneUpstreamCompressionDrafts(store.config.upstream_models);
    this.render(true);
  }
}

export function setupCompressionSettings(): void {
  const controls = [
    createControls("gemini", "gemini"),
    createControls("claude", "claude"),
    createControls("custom_model", "custom-model"),
  ];
  const resetButton = document.querySelector<HTMLButtonElement>("#reset-compression-settings");
  const saveButton = document.querySelector<HTMLButtonElement>("#save-compression-settings");
  const source = document.querySelector<HTMLElement>("#settings-compression-source");
  const modelList = document.querySelector<HTMLElement>("#compression-custom-model-list");
  const policyEditor = document.querySelector<HTMLElement>("#compression-policy-editor");
  const globalPolicyButton = document.querySelector<HTMLButtonElement>("#edit-global-checkpoint-policy");
  const policySaveButton = document.querySelector<HTMLButtonElement>("#save-checkpoint-policy");
  const jsonEditor = document.querySelector<HTMLTextAreaElement>("#compression-json-editor");
  const jsonEditButton = document.querySelector<HTMLButtonElement>("#compression-json-edit");
  const jsonFormatButton = document.querySelector<HTMLButtonElement>("#compression-json-format");
  const jsonSaveButton = document.querySelector<HTMLButtonElement>("#compression-json-save");
  const jsonStatus = document.querySelector<HTMLElement>("#compression-json-status");
  if (controls.some((control) => !control)
    || !resetButton
    || !saveButton
    || !source
    || !modelList
    || !policyEditor
    || !globalPolicyButton
    || !policySaveButton
    || !jsonEditor
    || !jsonEditButton
    || !jsonFormatButton
    || !jsonSaveButton
    || !jsonStatus) return;
  new CompressionSettingsController(
    controls as CompressionControls[],
    resetButton,
    saveButton,
    source,
    modelList,
    policyEditor,
    globalPolicyButton,
    policySaveButton,
    jsonEditor,
    jsonEditButton,
    jsonFormatButton,
    jsonSaveButton,
    jsonStatus,
  ).start();
}
