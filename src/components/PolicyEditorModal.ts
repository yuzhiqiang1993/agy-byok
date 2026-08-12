import { resolveEffectiveCompressionPolicy } from "../controllers/providerController";
import { t } from "../i18n";
import type { UpstreamCompressionPolicy } from "../types/catalog";
import type { ModelCompressionPolicy } from "../types/config";
import { errorMessage } from "../utils/errorUtils";
import { createModal, type ModalInstance } from "./common/Modal";

type CompressionPresetId = "CONTEXT_256K" | "CONTEXT_372K" | "CONTEXT_500K" | "CONTEXT_1M";

type PolicyMode = "NONE" | CompressionPresetId | "CUSTOM";
// 官方模型只覆盖三个 Token 阈值，自定义模型则完整注入当前策略。
type CompressionPolicyScope = "official_threshold_override" | "custom_full_policy";
type CompressionWorkerMode =
  | "CURRENT_MODEL"
  | "MODEL_PLACEHOLDER_M50"
  | "MODEL_PLACEHOLDER_M71"
  | "MODEL_PLACEHOLDER_M72";

interface CompressionWorkerPolicy {
  checkpointModel: string;
  useLastPlannerModel: boolean;
}

interface CompressionPreset {
  id: CompressionPresetId;
  labelKey:
    | "models.presetContext256K"
    | "models.presetContext372K"
    | "models.presetContext500K"
    | "models.presetContext1M";
  minCapacity: number;
  values: Pick<ModelCompressionPolicy, "token_threshold" | "max_token_limit" | "max_output_tokens">;
}

interface PolicyEditorModalOptions {
  scope: CompressionPolicyScope;
  modelName: string;
  currentPolicy: ModelCompressionPolicy | null;
  capacity: number | null;
  outputTokenLimit: number | null;
  defaultLabel: string;
  defaultHelp: string;
  emptyNotice: string;
  upstreamCompression?: UpstreamCompressionPolicy;
  focusKey: string;
  onSave: (policy: ModelCompressionPolicy | null) => Promise<void>;
}

const DEFAULT_OUTPUT_RESERVE = 16_384;
// 模型未声明输出上限时采用保守兜底；明确上限由模型自身决定。
const DEFAULT_MAX_OUTPUT_RESERVE = 65_536;

const DEFAULT_CHECKPOINT_MODEL = "MODEL_PLACEHOLDER_M71";
const FIXED_WORKER_MODES: Exclude<CompressionWorkerMode, "CURRENT_MODEL">[] = [
  "MODEL_PLACEHOLDER_M71",
  "MODEL_PLACEHOLDER_M72",
  "MODEL_PLACEHOLDER_M50",
];
const DEFAULT_POLICY_LIMITS = {
  token_threshold: 50_000,
  max_token_limit: 128_000,
  max_output_tokens: DEFAULT_OUTPUT_RESERVE,
};

const COMPRESSION_PRESETS: readonly CompressionPreset[] = [
  {
    id: "CONTEXT_256K",
    labelKey: "models.presetContext256K",
    minCapacity: 256_000,
    values: { token_threshold: 102_400, max_token_limit: 153_600, max_output_tokens: 30_720 },
  },
  {
    id: "CONTEXT_372K",
    labelKey: "models.presetContext372K",
    minCapacity: 372_000,
    values: { token_threshold: 148_800, max_token_limit: 223_200, max_output_tokens: 44_640 },
  },
  {
    id: "CONTEXT_500K",
    labelKey: "models.presetContext500K",
    minCapacity: 500_000,
    values: { token_threshold: 200_000, max_token_limit: 300_000, max_output_tokens: 60_000 },
  },
  {
    id: "CONTEXT_1M",
    labelKey: "models.presetContext1M",
    minCapacity: 1_000_000,
    values: { token_threshold: 419_430, max_token_limit: 629_145, max_output_tokens: 65_535 },
  },
];

const PRESET_IDS = COMPRESSION_PRESETS.map((preset) => preset.id);

function presetById(id: CompressionPresetId): CompressionPreset {
  return COMPRESSION_PRESETS.find((preset) => preset.id === id) ?? COMPRESSION_PRESETS[0];
}

function presetLabel(id: CompressionPresetId): string {
  return t(presetById(id).labelKey);
}

function presetSupported(
  preset: CompressionPreset,
  capacity: number | null,
  outputTokenLimit: number | null,
): boolean {
  if (capacity == null || capacity < preset.minCapacity) return false;
  return outputTokenLimit == null || preset.values.max_output_tokens <= outputTokenLimit;
}

function recommendedPresetForCapacity(
  capacity: number | null,
  outputTokenLimit?: number | null,
): CompressionPreset | null {
  if (capacity == null || capacity <= 0) return null;
  return (
    [...COMPRESSION_PRESETS]
      .reverse()
      .find((preset) => presetSupported(preset, capacity, outputTokenLimit ?? null)) ?? null
  );
}

function createPolicy(
  limits: Pick<ModelCompressionPolicy, "token_threshold" | "max_token_limit" | "max_output_tokens">,
  worker: CompressionWorkerPolicy,
): ModelCompressionPolicy {
  return {
    enabled: true,
    checkpoint_model: worker.checkpointModel,
    strategy: "CHECKPOINT_STRATEGY_UNSPECIFIED",
    max_overhead_ratio: "0.30",
    moving_window_size: "1",
    use_last_planner_model: worker.useLastPlannerModel,
    is_sync: false,
    max_user_requests: 10,
    include_last_user_message: false,
    include_conversation_log: true,
    include_running_task_snapshots: true,
    include_subagent_snapshots: true,
    include_artifact_snapshots: true,
    retry_config: {
      max_retries: 0,
      initial_sleep_duration_ms: 1_000,
      exponential_multiplier: 2,
      include_error_feedback: false,
    },
    ...limits,
  };
}

function createPresetPolicy(
  id: CompressionPresetId,
  worker: CompressionWorkerPolicy,
  baseline?: ModelCompressionPolicy | null,
): ModelCompressionPolicy {
  const values = presetById(id).values;
  return baseline
    ? { ...clonePolicy(baseline), ...values }
    : createPolicy(values, worker);
}

function matchingPreset(
  policy: ModelCompressionPolicy,
  capacity: number | null,
  outputTokenLimit: number | null,
): CompressionPresetId | null {
  if (!capacity || capacity <= 0) return null;
  const exact = COMPRESSION_PRESETS.find((preset) => (
    presetSupported(preset, capacity, outputTokenLimit)
      && policy.token_threshold === preset.values.token_threshold
      && policy.max_token_limit === preset.values.max_token_limit
      && policy.max_output_tokens === preset.values.max_output_tokens
  ))?.id;
  if (exact) return exact;
  return null;
}

function initialMode(
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
  outputTokenLimit: number | null,
): PolicyMode {
  if (!policy) return "NONE";
  return matchingPreset(policy, capacity, outputTokenLimit) ?? "CUSTOM";
}

function clonePolicy(policy: ModelCompressionPolicy): ModelCompressionPolicy {
  return {
    ...policy,
    retry_config: { ...policy.retry_config },
  };
}

function isFixedWorkerMode(value: string | undefined): value is Exclude<CompressionWorkerMode, "CURRENT_MODEL"> {
  return value !== undefined && FIXED_WORKER_MODES.includes(
    value as Exclude<CompressionWorkerMode, "CURRENT_MODEL">,
  );
}

function workerPolicyFrom(policy: ModelCompressionPolicy): CompressionWorkerPolicy {
  return {
    checkpointModel: isFixedWorkerMode(policy.checkpoint_model)
      ? policy.checkpoint_model
      : DEFAULT_CHECKPOINT_MODEL,
    useLastPlannerModel: policy.use_last_planner_model,
  };
}

function defaultWorkerPolicy(options: PolicyEditorModalOptions): CompressionWorkerPolicy {
  const upstream = options.upstreamCompression;
  return {
    checkpointModel: isFixedWorkerMode(upstream?.checkpointModel)
      ? upstream.checkpointModel
      : DEFAULT_CHECKPOINT_MODEL,
    useLastPlannerModel: upstream?.useLastPlannerModel ?? false,
  };
}


function formatTokenCount(value: number): string {
  if (value >= 1_000_000) {
    const millions = value / 1_000_000;
    return `${Number.isInteger(millions) ? millions : millions.toFixed(2).replace(/\.?0+$/, "")}M`;
  }
  if (value >= 1_000) {
    const thousands = value / 1_000;
    return `${Number.isInteger(thousands) ? thousands : thousands.toFixed(1).replace(/\.0$/, "")}K`;
  }
  return value.toLocaleString();
}

function isValidPolicy(
  policy: ModelCompressionPolicy,
  capacity: number | null,
  outputTokenLimit: number | null,
): boolean {
  const { token_threshold: threshold, max_token_limit: limit, max_output_tokens: output } = policy;
  const maximumOutputReserve = outputTokenLimit ?? DEFAULT_MAX_OUTPUT_RESERVE;
  return [threshold, limit, output].every((value) => Number.isSafeInteger(value) && value > 0)
    && output >= DEFAULT_OUTPUT_RESERVE
    && output <= maximumOutputReserve
    && threshold < limit
    && output < limit
    && threshold + output <= limit
    && (capacity == null || limit <= capacity)
    && (outputTokenLimit == null || output <= outputTokenLimit);
}

function createMetric(label: string, value: number, subtext?: string): HTMLDivElement {
  const metric = document.createElement("div");
  metric.className = "policy-metric";

  const name = document.createElement("span");
  name.textContent = label;

  const valueRow = document.createElement("div");
  const count = document.createElement("strong");
  count.textContent = value.toLocaleString();
  valueRow.append(count);

  if (subtext) {
    const badge = document.createElement("small");
    badge.textContent = subtext;
    valueRow.append(badge);
  }

  metric.append(name, valueRow);
  return metric;
}

function renderPolicyMetrics(
  container: HTMLElement,
  policy: ModelCompressionPolicy,
  capacity: number | null,
): void {
  const metrics = document.createElement("div");
  metrics.className = "policy-metric-grid";

  metrics.append(
    createMetric(t("models.policyThreshold"), policy.token_threshold, formatTokenCount(policy.token_threshold)),
    createMetric(t("models.policyMaxLimit"), policy.max_token_limit, formatTokenCount(policy.max_token_limit)),
    createMetric(t("models.policyMaxOutput"), policy.max_output_tokens, formatTokenCount(policy.max_output_tokens)),
  );
  container.append(metrics);
  renderCapacityBar(container, policy, capacity);
}

function renderCapacityBar(
  container: HTMLElement,
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
): void {
  if (!policy || !capacity || capacity <= 0) return;

  const threshold = policy.token_threshold;
  const limit = policy.max_token_limit;

  const thresholdPct = Math.min(100, Math.max(0, (threshold / capacity) * 100));
  const limitPct = Math.min(100, Math.max(0, (limit / capacity) * 100));
  const compressPct = Math.max(0, limitPct - thresholdPct);
  const reservePct = Math.max(0, 100 - limitPct);

  const wrapper = document.createElement("div");
  wrapper.className = "policy-capacity-bar-wrapper";

  const labelRow = document.createElement("div");
  labelRow.className = "policy-capacity-bar-labels";

  const title = document.createElement("span");
  title.className = "policy-bar-title";
  title.textContent = t("models.policyCapacityBarTitle");

  const legend = document.createElement("div");
  legend.className = "policy-bar-legend";

  const chatLegend = document.createElement("span");
  chatLegend.className = "legend-item legend-chat";
  chatLegend.textContent = t("models.policyContextChatSegment");

  const compressLegend = document.createElement("span");
  compressLegend.className = "legend-item legend-compress";
  compressLegend.textContent = t("models.policyContextCompressSegment");

  const reserveLegend = document.createElement("span");
  reserveLegend.className = "legend-item legend-reserve";
  reserveLegend.textContent = t("models.policyContextReserveSegment");

  legend.append(chatLegend, compressLegend, reserveLegend);
  labelRow.append(title, legend);

  const bar = document.createElement("div");
  bar.className = "policy-capacity-bar";

  const chatSegment = document.createElement("div");
  chatSegment.className = "bar-segment segment-chat";
  chatSegment.style.width = `${thresholdPct.toFixed(2)}%`;
  chatSegment.title = `${t("models.policyContextChatSegment")}: 0 ～ ${formatTokenCount(threshold)}`;

  const compressSegment = document.createElement("div");
  compressSegment.className = "bar-segment segment-compress";
  compressSegment.style.width = `${compressPct.toFixed(2)}%`;
  compressSegment.title = `${t("models.policyContextCompressSegment")}: ${formatTokenCount(threshold)} ～ ${formatTokenCount(limit)}`;

  const reserveSegment = document.createElement("div");
  reserveSegment.className = "bar-segment segment-reserve";
  reserveSegment.style.width = `${reservePct.toFixed(2)}%`;
  reserveSegment.title = `${t("models.policyContextReserveSegment")}: ${formatTokenCount(limit)} ～ ${formatTokenCount(capacity)}`;

  bar.append(chatSegment, compressSegment, reserveSegment);

  const ticksRow = document.createElement("div");
  ticksRow.className = "policy-capacity-ticks";

  const tickStart = document.createElement("span");
  tickStart.textContent = "0";

  const tickTrigger = document.createElement("span");
  tickTrigger.className = "tick-trigger";
  tickTrigger.textContent = `${formatTokenCount(threshold)}`;

  const tickLimit = document.createElement("span");
  tickLimit.className = "tick-limit";
  tickLimit.textContent = `${formatTokenCount(limit)}`;

  const tickCap = document.createElement("span");
  tickCap.className = "tick-cap";
  tickCap.textContent = `${formatTokenCount(capacity)}`;

  ticksRow.append(tickStart, tickTrigger, tickLimit, tickCap);

  wrapper.append(labelRow, bar, ticksRow);
  container.append(wrapper);
}


export function getPolicyPillStatus(
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
  outputTokenLimit: number | null,
  defaultLabel: string,
): { label: string; isManaged: boolean; tooltip: string } {
  if (!policy || !policy.enabled) {
    return {
      label: defaultLabel,
      isManaged: false,
      tooltip: t("models.policyPillTooltipDefault", { label: defaultLabel }),
    };
  }
  const preset = matchingPreset(policy, capacity, outputTokenLimit);
  const label = preset ? presetLabel(preset) : t("models.presetCustom");
  return {
    label,
    isManaged: true,
    tooltip: t("models.policyPillTooltip", {
      label,
      threshold: formatTokenCount(policy.token_threshold),
      limit: formatTokenCount(policy.max_token_limit),
      output: formatTokenCount(policy.max_output_tokens),
    }),
  };
}

export function showPolicyEditorModal(options: PolicyEditorModalOptions): void {
  const returnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;

  const body = document.createElement("div");
  body.className = "policy-editor-body";

  const defaultWorker = defaultWorkerPolicy(options);
  let mode = initialMode(options.currentPolicy, options.capacity, options.outputTokenLimit);
  let draft = options.currentPolicy ? clonePolicy(options.currentPolicy) : null;

  const source = document.createElement("p");
  source.className = "policy-source-note";

  const presetField = document.createElement("div");
  presetField.className = "policy-preset-field";
  const presetFieldLabel = document.createElement("span");
  presetFieldLabel.textContent = t("models.policySegmentedTitle");

  const presetSegmented = document.createElement("div");
  presetSegmented.className = "policy-preset-segmented";
  presetField.append(presetFieldLabel, presetSegmented);

  const help = document.createElement("p");
  help.className = "policy-help";

  const form = document.createElement("div");
  form.className = "policy-editor-form";

  const error = document.createElement("p");
  error.className = "policy-editor-error";
  error.setAttribute("role", "alert");
  error.tabIndex = -1;
  error.hidden = true;

  body.append(source, presetField, help, form, error);

  const renderSegmented = (): void => {
    presetSegmented.replaceChildren();
    const modes: Array<{ id: PolicyMode; label: string; disabled?: boolean; tooltip?: string }> = [
      { id: "NONE", label: options.defaultLabel },
      ...PRESET_IDS.map((id) => {
        const preset = presetById(id);
        const supported = presetSupported(preset, options.capacity, options.outputTokenLimit);
        return {
          id,
          label: presetLabel(id),
          disabled: !supported,
          tooltip: !supported
            ? options.capacity == null
              ? t("models.presetUnknownLimit")
              : t("models.presetUnsupported")
            : undefined,
        };
      }),
      { id: "CUSTOM", label: t("models.presetCustom") },
    ];

    const recommended = options.scope === "custom_full_policy"
      ? recommendedPresetForCapacity(options.capacity, options.outputTokenLimit)
      : null;

    for (const item of modes) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = `policy-pill-tab ${mode === item.id ? "active" : ""} ${item.disabled ? "disabled" : ""}`;

      const textSpan = document.createElement("span");
      textSpan.textContent = item.label;
      btn.append(textSpan);

      const isRecommended = options.scope === "official_threshold_override"
        ? item.id === "NONE"
        : recommended?.id === item.id;
      if (isRecommended) {
        const badge = document.createElement("span");
        badge.className = "policy-pill-recommend";
        badge.textContent = t("models.policyRecommended");
        btn.append(badge);
      }

      if (item.tooltip) btn.title = item.tooltip;
      btn.disabled = !!item.disabled;
      btn.addEventListener("click", () => {
        if (item.disabled || mode === item.id) return;
        mode = item.id;
        if (mode !== "NONE" && mode !== "CUSTOM") {
          const capacity = options.capacity;
          const preset = presetById(mode);
          if (presetSupported(preset, capacity, options.outputTokenLimit)) {
            const worker = draft ? workerPolicyFrom(draft) : defaultWorker;
            draft = createPresetPolicy(mode, worker, draft);
          }
        } else if (mode === "CUSTOM" && !draft) {
          const recPreset = recommendedPresetForCapacity(options.capacity, options.outputTokenLimit);
          draft = recPreset
            ? createPresetPolicy(recPreset.id, defaultWorker)
            : createPolicy(DEFAULT_POLICY_LIMITS, defaultWorker);
        }
        renderSegmented();
        render();
      });
      presetSegmented.append(btn);
    }
  };

  const renderDefaultPolicy = (): void => {
    help.textContent = options.defaultHelp;
    const upstream = options.upstreamCompression;
    if (upstream?.enabled && upstream.tokenThreshold > 0 && upstream.maxTokenLimit > 0) {
      const policy = createPolicy({
        token_threshold: upstream.tokenThreshold,
        max_token_limit: upstream.maxTokenLimit,
        max_output_tokens: upstream.maxOutputTokens && upstream.maxOutputTokens > 0
          ? upstream.maxOutputTokens
          : DEFAULT_OUTPUT_RESERVE,
      }, defaultWorker);
      renderPolicyMetrics(form, policy, options.capacity);
      return;
    }

    const empty = document.createElement("p");
    empty.className = "policy-empty-state";
    empty.textContent = options.emptyNotice;
    form.append(empty);
  };

  const createLimitInput = (
    labelText: string,
    field: "token_threshold" | "max_token_limit" | "max_output_tokens",
  ): HTMLDivElement => {
    const fieldContainer = document.createElement("div");
    fieldContainer.className = "policy-custom-field";

    const headerRow = document.createElement("div");
    headerRow.className = "policy-field-header";

    const text = document.createElement("span");
    text.className = "policy-field-title";
    text.textContent = labelText;

    const badge = document.createElement("span");
    badge.className = "policy-field-badge";

    headerRow.append(text, badge);

    const modeTabs = document.createElement("div");
    modeTabs.className = "policy-input-mode-segmented";

    const isOutputField = field === "max_output_tokens";
    const percentTab = document.createElement("button");
    percentTab.type = "button";
    percentTab.className = "policy-mode-pill active";
    percentTab.textContent = isOutputField ? t("models.policyInputByPreset") : t("models.policyInputByPercent");

    const tokenTab = document.createElement("button");
    tokenTab.type = "button";
    tokenTab.className = "policy-mode-pill";
    tokenTab.textContent = t("models.policyInputByTokens");

    modeTabs.append(percentTab, tokenTab);

    const inputWrapper = document.createElement("div");
    inputWrapper.className = "policy-input-wrapper";

    const input = document.createElement("input");
    input.type = "number";
    input.min = "1";
    input.step = "1";
    input.inputMode = "numeric";
    input.className = "policy-number-input";
    input.value = draft ? String(draft[field]) : "";

    const unit = document.createElement("span");
    unit.className = "policy-input-unit";
    unit.textContent = t("models.policyTokenUnit");

    inputWrapper.append(input, unit);

    const capacity = options.capacity;

    let updatePillHighlight = (): boolean => false;

    const updateBadge = () => {
      const val = Number(input.value);
      if (!val || isNaN(val)) {
        badge.textContent = "-";
        return;
      }
      badge.textContent = formatTokenCount(val);
    };

    const validateDraft = () => {
      if (!draft) return;
      if (!isValidPolicy(draft, options.capacity, options.outputTokenLimit)) {
        error.textContent = t("models.policyInvalid");
        error.hidden = false;
      } else {
        error.hidden = true;
      }
      renderMetricsInForm();
      updatePillHighlight();
      updateBadge();
    };

    let pillRow: HTMLDivElement;

    if (field === "max_output_tokens") {
      pillRow = document.createElement("div");
      pillRow.className = "policy-percentage-row policy-percentage-row-reserves";

      const reserves = [16_384, 32_768, 44_640, 65_535, 65_536];
      const pillButtons: HTMLButtonElement[] = [];

      for (const val of reserves) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "policy-percentage-btn";
        btn.textContent = formatTokenCount(val);
        btn.title = formatTokenCount(val);
        if (options.outputTokenLimit && val > options.outputTokenLimit) {
          btn.disabled = true;
          btn.classList.add("disabled");
        }
        btn.addEventListener("click", () => {
          if (!draft) return;
          draft[field] = val;
          input.value = String(val);
          validateDraft();
        });
        pillButtons.push(btn);
        pillRow.append(btn);
      }

      updatePillHighlight = () => {
        const current = draft?.[field];
        let hasMatch = false;
        for (let i = 0; i < reserves.length; i++) {
          const isMatch = current === reserves[i];
          pillButtons[i].classList.toggle("active", isMatch);
          if (isMatch) hasMatch = true;
        }
        return hasMatch;
      };
    } else {
      const percentages = field === "token_threshold"
        ? [20, 30, 40, 50, 60, 70, 80]
        : [40, 50, 60, 70, 80, 90, 95];

      pillRow = document.createElement("div");
      pillRow.className = "policy-percentage-row";
      const pillButtons: { btn: HTMLButtonElement; pct: number }[] = [];

      if (capacity && capacity > 0) {
        for (const pct of percentages) {
          const btn = document.createElement("button");
          btn.type = "button";
          btn.className = "policy-percentage-btn";
          btn.textContent = `${pct}%`;
          const calculatedTokens = Math.round(capacity * (pct / 100));
          btn.title = formatTokenCount(calculatedTokens);

          btn.addEventListener("click", () => {
            if (!draft) return;
            draft[field] = calculatedTokens;
            input.value = String(calculatedTokens);
            validateDraft();
          });

          pillButtons.push({ btn, pct });
          pillRow.append(btn);
        }
      }

      updatePillHighlight = () => {
        if (!capacity || capacity <= 0 || !draft) return false;
        const currentVal = draft[field];
        const currentPct = (currentVal / capacity) * 100;
        let hasMatch = false;
        for (const { btn, pct } of pillButtons) {
          const isMatch = Math.abs(currentPct - pct) < 2.5;
          btn.classList.toggle("active", isMatch);
          if (isMatch) hasMatch = true;
        }
        return hasMatch;
      };
    }

    const setMode = (isPercentMode: boolean) => {
      percentTab.classList.toggle("active", isPercentMode);
      tokenTab.classList.toggle("active", !isPercentMode);
      pillRow.hidden = !isPercentMode;
      inputWrapper.hidden = isPercentMode;
    };

    percentTab.addEventListener("click", () => setMode(true));
    tokenTab.addEventListener("click", () => setMode(false));

    input.addEventListener("input", () => {
      if (!draft) return;
      draft[field] = Number(input.value);
      validateDraft();
    });

    const isMatched = updatePillHighlight();
    setMode(isMatched);

    fieldContainer.append(headerRow, modeTabs, pillRow, inputWrapper);
    updateBadge();
    return fieldContainer;
  };

  const renderMetricsInForm = () => {
    const existingBar = form.querySelector(".policy-capacity-bar-wrapper");
    if (existingBar) existingBar.remove();
    const existingMetrics = form.querySelector(".policy-metric-grid");
    if (existingMetrics) existingMetrics.remove();
    if (draft) renderPolicyMetrics(form, draft, options.capacity);
  };

  const render = (): void => {
    form.replaceChildren();
    error.hidden = true;
    source.textContent = options.scope === "official_threshold_override"
      ? mode === "NONE"
        ? t("models.policySourceOfficialDefault")
        : t("models.policySourceOfficialOverride")
      : mode === "NONE"
        ? t("models.policySourceUpstreamDefault")
        : t("models.policySourceByokFull");
    if (mode === "NONE") {
      renderDefaultPolicy();
      return;
    }

    help.textContent = mode === "CUSTOM" ? t("models.policyCustomHelp") : t("models.policyPresetHelp");
    if (!draft) {
      draft = createPolicy(DEFAULT_POLICY_LIMITS, defaultWorker);
    }

    if (mode === "CUSTOM") {
      const limits = document.createElement("div");
      limits.className = "policy-custom-grid";
      limits.append(
        createLimitInput(t("models.policyThreshold"), "token_threshold"),
        createLimitInput(t("models.policyMaxLimit"), "max_token_limit"),
        createLimitInput(t("models.policyMaxOutput"), "max_output_tokens"),
      );
      form.append(limits);
      renderPolicyMetrics(form, draft, options.capacity);
    } else {
      renderPolicyMetrics(form, draft, options.capacity);
    }

  };

  renderSegmented();
  render();

  let modal: ModalInstance;

  const setSaving = (saving: boolean): void => {
    modal.setBusy(saving, t("models.saving"));
  };

  const titleExtras: HTMLElement[] = [];
  const modelBadge = document.createElement("span");
  modelBadge.className = "policy-model-badge";
  modelBadge.textContent = options.modelName;
  titleExtras.push(modelBadge);

  if (options.capacity && options.capacity > 0) {
    const capacityBadge = document.createElement("span");
    capacityBadge.className = "policy-capacity-badge";
    capacityBadge.textContent = `${t("models.policyContextWindow")} · ${formatTokenCount(options.capacity)}`;
    titleExtras.push(capacityBadge);
  }

  modal = createModal({
    title: t("models.editPolicyTitle"),
    titleExtras,
    body,
    dialogClassName: "policy-editor-dialog",
    okLabel: t("common.save"),
    cancelLabel: t("common.cancel"),
    onOk: () => {
      error.hidden = true;
      const nextPolicy = mode === "NONE" ? null : draft;
      if (nextPolicy && !isValidPolicy(nextPolicy, options.capacity, options.outputTokenLimit)) {
        error.textContent = t("models.policyInvalid");
        error.hidden = false;
        return;
      }

      setSaving(true);
      const resolvedPolicy = nextPolicy
        ? resolveEffectiveCompressionPolicy(
            clonePolicy(nextPolicy),
            options.capacity,
            options.outputTokenLimit,
          )
        : Promise.resolve(null);
      void resolvedPolicy
        .then((policy) => options.onSave(policy))
        .then(() => {
          modal.close();
        })
        .catch((saveError: unknown) => {
          setSaving(false);
          error.textContent = t("models.policySaveFailed", { message: errorMessage(saveError) });
          error.hidden = false;
          error.focus();
        });
    },
    onClosed: () => {
      if (returnFocus?.isConnected) return;
      const replacement = [...document.querySelectorAll<HTMLElement>("[data-policy-focus-key]")]
        .find((element) => element.dataset.policyFocusKey === options.focusKey);
      const focusTarget = replacement ?? document.querySelector<HTMLElement>(".provider-tab-card.active");
      if (focusTarget?.isConnected) window.setTimeout(() => focusTarget.focus(), 0);
    },
  });

  render();
  const activeTab = presetSegmented.querySelector<HTMLElement>(".policy-pill-tab.active") ?? presetSegmented;
  window.setTimeout(() => activeTab.focus(), 0);
}
