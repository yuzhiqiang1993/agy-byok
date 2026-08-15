import { resolveEffectiveCompressionPolicy } from "../controllers/providerController";
import { t } from "../i18n";
import type { UpstreamCompressionPolicy } from "../types/catalog";
import type { ModelCompressionPolicy } from "../types/config";
import { errorMessage } from "../utils/errorUtils";
import { createModal, type ModalInstance } from "./common/Modal";
import {
  type CompressionPolicyScope,
  type PolicyMode,
  DEFAULT_POLICY_LIMITS,
  PRESET_IDS,
  clonePolicy,
  createPolicy,
  createPresetPolicy,
  defaultWorkerPolicy,
  formatTokenCount,
  initialMode,
  isValidPolicy,
  presetById,
  presetLabel,
  presetSupported,
  recommendedPresetForCapacity,
  workerPolicyFrom,
} from "./policy/policyPresets";
import {
  getPolicyPillStatus,
  renderPolicyMetrics,
} from "./policy/policyVisuals";

export { getPolicyPillStatus };

export interface PolicyEditorModalOptions {
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

export function showPolicyEditorModal(options: PolicyEditorModalOptions): void {
  const returnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null;

  const body = document.createElement("div");
  body.className = "policy-editor-body";

  const defaultWorker = defaultWorkerPolicy(options.upstreamCompression);
  let mode: PolicyMode = initialMode(options.currentPolicy, options.capacity, options.outputTokenLimit);
  let draft = options.currentPolicy ? clonePolicy(options.currentPolicy) : null;

  const presetField = document.createElement("div");
  presetField.className = "policy-preset-field";

  const presetSegmented = document.createElement("div");
  presetSegmented.className = "policy-preset-segmented";
  presetField.append(presetSegmented);

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
          : DEFAULT_POLICY_LIMITS.max_output_tokens,
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

      const reserves = [16_384, 32_768, 44_640, 65_535];
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

  const renderWorkerStrategy = (container: HTMLElement) => {
    if (!draft) return;

    const strategyField = document.createElement("div");
    strategyField.className = "policy-preset-field";
    strategyField.style.marginTop = "16px";

    const label = document.createElement("span");
    label.textContent = t("models.policyWorkerStrategy");

    const segmented = document.createElement("div");
    segmented.className = "policy-preset-segmented";

    const isSameModel = draft.strategy === "CHECKPOINT_STRATEGY_SAME_MODEL" && draft.use_last_planner_model;

    const sameModelBtn = document.createElement("button");
    sameModelBtn.type = "button";
    sameModelBtn.className = `policy-pill-tab ${isSameModel ? "active" : ""}`;
    sameModelBtn.textContent = t("models.policyStrategySameModel");
    sameModelBtn.onclick = () => {
      if (!draft) return;
      draft.strategy = "CHECKPOINT_STRATEGY_SAME_MODEL";
      draft.use_last_planner_model = true;
      render();
    };

    const defaultBtn = document.createElement("button");
    defaultBtn.type = "button";
    defaultBtn.className = `policy-pill-tab ${!isSameModel ? "active" : ""}`;
    defaultBtn.textContent = t("models.policyStrategyDefault");
    defaultBtn.onclick = () => {
      if (!draft) return;
      draft.strategy = "CHECKPOINT_STRATEGY_UNSPECIFIED";
      draft.use_last_planner_model = false;
      render();
    };

    segmented.append(sameModelBtn, defaultBtn);
    strategyField.append(label, segmented);
    container.append(strategyField);
  };

  const render = (): void => {
    form.replaceChildren();
    error.hidden = true;

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
    }

    renderWorkerStrategy(form);
    renderPolicyMetrics(form, draft, options.capacity);
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
