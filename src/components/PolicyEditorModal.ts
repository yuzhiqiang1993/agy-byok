import { t } from "../i18n";
import type { UpstreamCompressionPolicy } from "../types/catalog";
import type { ModelCompressionPolicy } from "../types/config";
import { errorMessage } from "../utils/errorUtils";
import { createModal } from "./common/Modal";

type CompressionPresetId =
  | "EXTREMELY_CONSERVATIVE"
  | "CONSERVATIVE"
  | "SLIGHTLY_CONSERVATIVE"
  | "BALANCED"
  | "AGGRESSIVE"
  | "EXTREMELY_AGGRESSIVE";

type PolicyMode = "NONE" | CompressionPresetId | "CUSTOM";
type CompressionWorkerMode =
  | "CURRENT_MODEL"
  | "MODEL_PLACEHOLDER_M50"
  | "MODEL_PLACEHOLDER_M71"
  | "MODEL_PLACEHOLDER_M72";

interface CompressionWorkerPolicy {
  checkpointModel: string;
  useLastPlannerModel: boolean;
}

interface PresetRatio {
  threshold: number;
  limit: number;
  output: number;
}

interface PolicyEditorModalOptions {
  modelName: string;
  currentPolicy: ModelCompressionPolicy | null;
  capacity: number | null;
  outputTokenLimit: number | null;
  defaultLabel: string;
  defaultHelp: string;
  emptyNotice: string;
  upstreamCompression?: UpstreamCompressionPolicy;
  preferCurrentWorker: boolean;
  focusKey: string;
  onSave: (policy: ModelCompressionPolicy | null) => Promise<void>;
}

const DEFAULT_OUTPUT_RESERVE = 16_384;
const DEFAULT_CHECKPOINT_MODEL = "MODEL_PLACEHOLDER_M71";
const FIXED_WORKER_MODES: Exclude<CompressionWorkerMode, "CURRENT_MODEL">[] = [
  "MODEL_PLACEHOLDER_M71",
  "MODEL_PLACEHOLDER_M72",
  "MODEL_PLACEHOLDER_M50",
];
const DEFAULT_POLICY_LIMITS = {
  token_threshold: 61_000,
  max_token_limit: 73_000,
  max_output_tokens: 2_000,
};

const PRESET_RATIOS: Record<CompressionPresetId, PresetRatio> = {
  EXTREMELY_CONSERVATIVE: { threshold: 0.3, limit: 0.5, output: 0.15 },
  CONSERVATIVE: { threshold: 0.4, limit: 0.6, output: 0.12 },
  SLIGHTLY_CONSERVATIVE: { threshold: 0.5, limit: 0.7, output: 0.10 },
  BALANCED: { threshold: 0.6, limit: 0.8, output: 0.08 },
  AGGRESSIVE: { threshold: 0.7, limit: 0.85, output: 0.05 },
  EXTREMELY_AGGRESSIVE: { threshold: 0.8, limit: 0.95, output: 0.02 },
};

const PRESET_IDS = Object.keys(PRESET_RATIOS) as CompressionPresetId[];

function presetLabel(id: CompressionPresetId): string {
  switch (id) {
    case "EXTREMELY_CONSERVATIVE":
      return t("models.presetExtremelyConservative");
    case "CONSERVATIVE":
      return t("models.presetConservative");
    case "SLIGHTLY_CONSERVATIVE":
      return t("models.presetSlightlyConservative");
    case "BALANCED":
      return t("models.presetBalanced");
    case "AGGRESSIVE":
      return t("models.presetAggressive");
    case "EXTREMELY_AGGRESSIVE":
      return t("models.presetExtremelyAggressive");
  }
}

function presetValues(
  id: CompressionPresetId,
  capacity: number,
  outputTokenLimit: number | null,
): Pick<ModelCompressionPolicy, "token_threshold" | "max_token_limit" | "max_output_tokens"> {
  const ratio = PRESET_RATIOS[id];
  const outputLimit = outputTokenLimit && outputTokenLimit > 0
    ? outputTokenLimit
    : DEFAULT_OUTPUT_RESERVE;
  return {
    token_threshold: Math.floor(capacity * ratio.threshold),
    max_token_limit: Math.floor(capacity * ratio.limit),
    max_output_tokens: Math.floor(Math.min(capacity * ratio.output, outputLimit)),
  };
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
  capacity: number,
  outputTokenLimit: number | null,
  worker: CompressionWorkerPolicy,
): ModelCompressionPolicy {
  return createPolicy(presetValues(id, capacity, outputTokenLimit), worker);
}

function matchingPreset(
  policy: ModelCompressionPolicy,
  capacity: number | null,
  outputTokenLimit: number | null,
): CompressionPresetId | null {
  if (!capacity || capacity <= 0) return null;
  return PRESET_IDS.find((id) => {
    const values = presetValues(id, capacity, outputTokenLimit);
    return policy.token_threshold === values.token_threshold
      && policy.max_token_limit === values.max_token_limit
      && policy.max_output_tokens === values.max_output_tokens;
  }) ?? null;
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
    useLastPlannerModel: upstream?.useLastPlannerModel ?? options.preferCurrentWorker,
  };
}

function workerMode(policy: ModelCompressionPolicy): CompressionWorkerMode {
  if (policy.use_last_planner_model) return "CURRENT_MODEL";
  return isFixedWorkerMode(policy.checkpoint_model)
    ? policy.checkpoint_model
    : DEFAULT_CHECKPOINT_MODEL;
}

function applyWorkerMode(policy: ModelCompressionPolicy, mode: CompressionWorkerMode): void {
  // “跟随当前模型”由官方字段控制；checkpoint_model 仍保留合法占位模型。
  if (mode === "CURRENT_MODEL") {
    policy.use_last_planner_model = true;
    if (!isFixedWorkerMode(policy.checkpoint_model)) {
      policy.checkpoint_model = DEFAULT_CHECKPOINT_MODEL;
    }
    return;
  }
  policy.use_last_planner_model = false;
  policy.checkpoint_model = mode;
}

function workerModeLabel(mode: CompressionWorkerMode): string {
  switch (mode) {
    case "CURRENT_MODEL":
      return t("models.policyWorkerCurrentModel");
    case "MODEL_PLACEHOLDER_M50":
      return t("models.policyWorkerModelM50");
    case "MODEL_PLACEHOLDER_M71":
      return t("models.policyWorkerModelM71");
    case "MODEL_PLACEHOLDER_M72":
      return t("models.policyWorkerModelM72");
  }
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

function isValidPolicy(policy: ModelCompressionPolicy): boolean {
  const { token_threshold: threshold, max_token_limit: limit, max_output_tokens: output } = policy;
  return [threshold, limit, output].every((value) => Number.isSafeInteger(value) && value > 0)
    && threshold < limit
    && output < limit
    && threshold + output <= limit;
}

function createMetric(label: string, value: number, ratio?: number): HTMLDivElement {
  const metric = document.createElement("div");
  metric.className = "policy-metric";

  const name = document.createElement("span");
  name.textContent = label;

  const valueRow = document.createElement("div");
  const count = document.createElement("strong");
  count.textContent = value.toLocaleString();
  valueRow.append(count);

  if (ratio !== undefined) {
    const badge = document.createElement("small");
    badge.textContent = `${Math.round(ratio * 100)}%`;
    valueRow.append(badge);
  }

  metric.append(name, valueRow);
  return metric;
}

function renderPolicyMetrics(
  container: HTMLElement,
  policy: ModelCompressionPolicy,
  ratio?: PresetRatio,
): void {
  const metrics = document.createElement("div");
  metrics.className = "policy-metric-grid";
  metrics.append(
    createMetric(t("models.policyThreshold"), policy.token_threshold, ratio?.threshold),
    createMetric(t("models.policyMaxLimit"), policy.max_token_limit, ratio?.limit),
    createMetric(t("models.policyMaxOutput"), policy.max_output_tokens, ratio?.output),
  );
  container.append(metrics);
}

function renderWorkerModel(container: HTMLElement, modelName: string): void {
  const worker = document.createElement("div");
  worker.className = "policy-worker-row";

  const label = document.createElement("span");
  label.textContent = t("models.policyCheckpointModel");

  const value = document.createElement("strong");
  value.textContent = modelName;

  worker.append(label, value);
  container.append(worker);
}

function renderWorkerModelSelect(
  container: HTMLElement,
  policy: ModelCompressionPolicy,
  onChange: () => void,
): void {
  const field = document.createElement("label");
  field.className = "policy-worker-row";

  const label = document.createElement("span");
  label.textContent = t("models.policyCheckpointModel");

  const select = document.createElement("select");
  for (const mode of ["CURRENT_MODEL", ...FIXED_WORKER_MODES] as CompressionWorkerMode[]) {
    const option = document.createElement("option");
    option.value = mode;
    option.textContent = workerModeLabel(mode);
    select.append(option);
  }
  select.value = workerMode(policy);
  select.addEventListener("change", () => {
    applyWorkerMode(policy, select.value as CompressionWorkerMode);
    onChange();
  });

  field.append(label, select);
  container.append(field);
}

export function getPolicyPillStatus(
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
  outputTokenLimit: number | null,
  defaultLabel: string,
): { label: string; isManaged: boolean } {
  if (!policy) return { label: defaultLabel, isManaged: false };
  const preset = matchingPreset(policy, capacity, outputTokenLimit);
  return {
    label: preset ? presetLabel(preset) : t("models.presetCustom"),
    isManaged: true,
  };
}

export function showPolicyEditorModal(options: PolicyEditorModalOptions): void {
  const returnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;

  const body = document.createElement("div");
  body.className = "policy-editor-body";

  const presetField = document.createElement("label");
  presetField.className = "policy-preset-field";
  const presetFieldLabel = document.createElement("span");
  presetFieldLabel.textContent = t("models.policyPresetTitle");
  const presetSelect = document.createElement("select");

  const noneOption = document.createElement("option");
  noneOption.value = "NONE";
  noneOption.textContent = options.defaultLabel;
  presetSelect.append(noneOption);

  for (const id of PRESET_IDS) {
    const ratio = PRESET_RATIOS[id];
    const option = document.createElement("option");
    option.value = id;
    option.textContent = `${presetLabel(id)} · ${Math.round(ratio.threshold * 100)}% / ${Math.round(ratio.limit * 100)}% / ${Math.round(ratio.output * 100)}%`;
    if (!options.capacity || options.capacity <= 0) {
      option.disabled = true;
      option.textContent = `${option.textContent} · ${t("models.presetUnknownLimit")}`;
    }
    presetSelect.append(option);
  }

  const customOption = document.createElement("option");
  customOption.value = "CUSTOM";
  customOption.textContent = t("models.presetCustom");
  presetSelect.append(customOption);
  presetField.append(presetFieldLabel, presetSelect);

  const help = document.createElement("p");
  help.className = "policy-help";

  const form = document.createElement("div");
  form.className = "policy-editor-form";

  const error = document.createElement("p");
  error.className = "policy-editor-error";
  error.setAttribute("role", "alert");
  error.tabIndex = -1;
  error.hidden = true;

  body.append(presetField, help, form, error);

  const defaultWorker = defaultWorkerPolicy(options);
  let mode = initialMode(options.currentPolicy, options.capacity, options.outputTokenLimit);
  let draft = options.currentPolicy ? clonePolicy(options.currentPolicy) : null;
  presetSelect.value = mode;


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
      const capacity = options.capacity && options.capacity > 0 ? options.capacity : null;
      const ratios = capacity
        ? {
            threshold: policy.token_threshold / capacity,
            limit: policy.max_token_limit / capacity,
            output: policy.max_output_tokens / capacity,
          }
        : undefined;
      renderPolicyMetrics(form, policy, ratios);
      renderWorkerModel(form, workerModeLabel(workerMode(policy)));
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
  ): HTMLLabelElement => {
    const label = document.createElement("label");
    const text = document.createElement("span");
    text.textContent = labelText;
    const input = document.createElement("input");
    input.type = "number";
    input.min = "1";
    input.step = "1";
    input.inputMode = "numeric";
    input.value = draft ? String(draft[field]) : "";
    input.addEventListener("input", () => {
      if (!draft) return;
      draft[field] = Number(input.value);
      error.hidden = true;
    });
    label.append(text, input);
    return label;
  };

  const render = (): void => {
    form.replaceChildren();
    error.hidden = true;
    if (mode === "NONE") {
      renderDefaultPolicy();
      return;
    }

    help.textContent = t("models.policyPresetHelp");
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
    } else {
      renderPolicyMetrics(form, draft, PRESET_RATIOS[mode]);
    }
    renderWorkerModelSelect(form, draft, () => {
      error.hidden = true;
    });

    const workerHelp = document.createElement("p");
    workerHelp.className = "field-hint policy-worker-help";
    workerHelp.textContent = t("models.policyCheckpointModelHelp");
    form.append(workerHelp);
  };

  presetSelect.addEventListener("change", () => {
    mode = presetSelect.value as PolicyMode;
    if (mode !== "NONE" && mode !== "CUSTOM") {
      const capacity = options.capacity;
      if (capacity && capacity > 0) {
        const worker = draft ? workerPolicyFrom(draft) : defaultWorker;
        draft = createPresetPolicy(mode, capacity, options.outputTokenLimit, worker);
      }
    } else if (mode === "CUSTOM" && !draft) {
      const capacity = options.capacity;
      draft = capacity && capacity > 0
        ? createPresetPolicy(
            "SLIGHTLY_CONSERVATIVE",
            capacity,
            options.outputTokenLimit,
            defaultWorker,
          )
        : createPolicy(DEFAULT_POLICY_LIMITS, defaultWorker);
    }
    render();
  });

  let modal: any;

  const setSaving = (saving: boolean): void => {
    for (const control of body.querySelectorAll<HTMLButtonElement | HTMLInputElement | HTMLSelectElement>("button, input, select")) {
      control.disabled = saving;
    }
    const saveBtn = modal.element.querySelector(".agy-modal-footer .primary");
    if (saveBtn) {
       saveBtn.textContent = saving ? t("models.saving") : t("common.save");
    }
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
    subtitle: t("models.editPolicyDesc"),
    titleExtras,
    body,
    dialogClassName: "policy-editor-dialog",
    okLabel: t("common.save"),
    cancelLabel: t("common.cancel"),
    onOk: () => {
      error.hidden = true;
      const nextPolicy = mode === "NONE" ? null : draft;
      if (nextPolicy && !isValidPolicy(nextPolicy)) {
        error.textContent = t("models.policyInvalid");
        error.hidden = false;
        return;
      }

      setSaving(true);
      void options.onSave(nextPolicy ? clonePolicy(nextPolicy) : null)
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
    onCancel: () => {
        // Just close
    }
  });

  const originalClose = modal.close;
  modal.close = () => {
      originalClose();
      if (returnFocus?.isConnected) window.setTimeout(() => returnFocus.focus(), 0);
  };

  render();
  window.setTimeout(() => presetSelect.focus(), 0);
}

