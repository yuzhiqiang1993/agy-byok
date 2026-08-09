import { t } from "../i18n";
import type { UpstreamCompressionPolicy } from "../types/catalog";
import type { ModelCompressionPolicy } from "../types/config";
import { visibleFocusableElements } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";

type CompressionPresetId =
  | "EXTREMELY_CONSERVATIVE"
  | "CONSERVATIVE"
  | "SLIGHTLY_CONSERVATIVE"
  | "BALANCED"
  | "AGGRESSIVE"
  | "EXTREMELY_AGGRESSIVE";

type PolicyMode = "NONE" | CompressionPresetId | "CUSTOM";

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
  focusKey: string;
  onSave: (policy: ModelCompressionPolicy | null) => Promise<void>;
}

const DEFAULT_OUTPUT_RESERVE = 16_384;
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
): ModelCompressionPolicy {
  return {
    enabled: true,
    checkpoint_model: "MODEL_PLACEHOLDER_M71",
    strategy: "CHECKPOINT_STRATEGY_UNSPECIFIED",
    max_overhead_ratio: "0.30",
    moving_window_size: "1",
    use_last_planner_model: false,
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
): ModelCompressionPolicy {
  return createPolicy(presetValues(id, capacity, outputTokenLimit));
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
  const overlay = document.createElement("div");
  overlay.className = "provider-modal";

  const backdrop = document.createElement("div");
  backdrop.className = "provider-modal-backdrop";

  const dialog = document.createElement("section");
  dialog.className = "provider-modal-dialog policy-editor-dialog";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.tabIndex = -1;
  dialog.setAttribute("aria-labelledby", "policy-editor-title");
  dialog.setAttribute("aria-describedby", "policy-editor-description");

  const header = document.createElement("header");
  header.className = "provider-modal-header";

  const heading = document.createElement("div");
  heading.className = "policy-editor-heading";

  const titleRow = document.createElement("div");
  titleRow.className = "policy-editor-title-row";
  const title = document.createElement("strong");
  title.id = "policy-editor-title";
  title.textContent = t("models.editPolicyTitle");
  const modelBadge = document.createElement("span");
  modelBadge.className = "policy-model-badge";
  modelBadge.textContent = options.modelName;
  titleRow.append(title, modelBadge);

  if (options.capacity && options.capacity > 0) {
    const capacityBadge = document.createElement("span");
    capacityBadge.className = "policy-capacity-badge";
    capacityBadge.textContent = `${t("models.policyContextWindow")} · ${formatTokenCount(options.capacity)}`;
    titleRow.append(capacityBadge);
  }

  const description = document.createElement("p");
  description.id = "policy-editor-description";
  description.textContent = t("models.editPolicyDesc");
  heading.append(titleRow, description);

  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "provider-modal-close";
  closeButton.setAttribute("aria-label", t("modal.close"));
  closeButton.title = t("modal.closeWithShortcut");
  closeButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`;
  header.append(heading, closeButton);

  const body = document.createElement("div");
  body.className = "provider-modal-body policy-editor-body";

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

  const footer = document.createElement("footer");
  footer.className = "reasoning-modal-footer";
  const cancelButton = document.createElement("button");
  cancelButton.type = "button";
  cancelButton.className = "secondary";
  cancelButton.textContent = t("common.cancel");
  const saveButton = document.createElement("button");
  saveButton.type = "button";
  saveButton.className = "primary";
  saveButton.textContent = t("common.save");
  footer.append(cancelButton, saveButton);

  dialog.append(header, body, footer);
  overlay.append(backdrop, dialog);

  let mode = initialMode(options.currentPolicy, options.capacity, options.outputTokenLimit);
  let draft = options.currentPolicy ? clonePolicy(options.currentPolicy) : null;
  let isSaving = false;
  presetSelect.value = mode;

  const close = (): void => {
    if (isSaving) return;
    window.removeEventListener("keydown", handleKeyDown);
    document.body.classList.remove("modal-open");
    overlay.remove();
    const replacement = [...document.querySelectorAll<HTMLElement>("[data-policy-focus-key]")]
      .find((element) => element.dataset.policyFocusKey === options.focusKey);
    const focusTarget = returnFocus?.isConnected
      ? returnFocus
      : replacement ?? document.querySelector<HTMLElement>(".provider-tab-card.active");
    if (focusTarget?.isConnected) window.setTimeout(() => focusTarget.focus(), 0);
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
      });
      const capacity = options.capacity && options.capacity > 0 ? options.capacity : null;
      const ratios = capacity
        ? {
            threshold: policy.token_threshold / capacity,
            limit: policy.max_token_limit / capacity,
            output: policy.max_output_tokens / capacity,
          }
        : undefined;
      renderPolicyMetrics(form, policy, ratios);
      renderWorkerModel(form, upstream.checkpointModel ?? t("models.policyUpstreamWorker"));
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
      draft = createPolicy(DEFAULT_POLICY_LIMITS);
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
    renderWorkerModel(form, t("models.policyWorkerModelM71"));

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
        draft = createPresetPolicy(mode, capacity, options.outputTokenLimit);
      }
    } else if (mode === "CUSTOM" && !draft) {
      const capacity = options.capacity;
      draft = capacity && capacity > 0
        ? createPresetPolicy("SLIGHTLY_CONSERVATIVE", capacity, options.outputTokenLimit)
        : createPolicy(DEFAULT_POLICY_LIMITS);
    }
    render();
  });

  const setSaving = (saving: boolean): void => {
    isSaving = saving;
    for (const control of dialog.querySelectorAll<HTMLButtonElement | HTMLInputElement | HTMLSelectElement>("button, input, select")) {
      control.disabled = saving;
    }
    saveButton.textContent = saving ? t("models.saving") : t("common.save");
    if (saving) dialog.focus();
  };

  saveButton.addEventListener("click", () => {
    error.hidden = true;
    const nextPolicy = mode === "NONE" ? null : draft;
    if (nextPolicy && !isValidPolicy(nextPolicy)) {
      error.textContent = t("models.policyInvalid");
      error.hidden = false;
      return;
    }

    setSaving(true);
    void options.onSave(nextPolicy ? createPolicy({
      token_threshold: nextPolicy.token_threshold,
      max_token_limit: nextPolicy.max_token_limit,
      max_output_tokens: nextPolicy.max_output_tokens,
    }) : null)
      .then(() => {
        isSaving = false;
        close();
      })
      .catch((saveError: unknown) => {
        setSaving(false);
        error.textContent = t("models.policySaveFailed", { message: errorMessage(saveError) });
        error.hidden = false;
        error.focus();
      });
  });

  cancelButton.addEventListener("click", close);
  closeButton.addEventListener("click", close);
  backdrop.addEventListener("click", close);

  function handleKeyDown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = visibleFocusableElements(dialog);
    if (focusable.length === 0) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialog.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }

  render();
  document.body.append(overlay);
  document.body.classList.add("modal-open");
  window.addEventListener("keydown", handleKeyDown);
  window.setTimeout(() => presetSelect.focus(), 0);
}
