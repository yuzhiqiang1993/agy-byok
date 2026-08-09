import { updateConfig } from "../controllers/configController";
import { fetchOfficialModels } from "../controllers/providerController";
import { t } from "../i18n";
import { store } from "../store/appStore";
import type { ProviderCatalogModel } from "../types/catalog";
import { errorMessage } from "../utils/errorUtils";
import { reasoningLevelLabel } from "../utils/reasoningUtils";
import { showNotice } from "./NoticeBar";
import { getPolicyPillStatus, showPolicyEditorModal } from "./PolicyEditorModal";
import { showRawConfigModal } from "./RawConfigModal";

const COPY_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
const COPIED_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;

const OFFICIAL_ENDPOINT_URL = "https://daily-cloudcode-pa.googleapis.com";

// 官方客户端当前公开的 11 个主力 Agent 模型；保持与 IDE、App、CLI 目录交集一致。
const MAIN_AGENT_MODEL_IDS = [
  "gemini-3.6-flash-high",
  "gemini-3.6-flash-medium",
  "gemini-3.6-flash-low",
  "gemini-3.5-flash-high",
  "gemini-3.5-flash-medium",
  "gemini-3.5-flash-low",
  "gemini-3.1-pro-high",
  "gemini-3.1-pro-low",
  "claude-sonnet-4-6",
  "claude-opus-4-6-thinking",
  "gpt-oss-120b-medium",
];

// 纯粹根据原始数据 item.reasoning.levels 渲染等级 Pill；无数据或无有效等级时直接渲染默认
function createReasoningVariantPills(item: ProviderCatalogModel): HTMLElement {
  const container = document.createElement("div");
  container.className = "provider-model-variants-inline";

  const levels = item.reasoning?.levels;
  if (Array.isArray(levels) && levels.length > 0) {
    for (const level of levels) {
      if (level === "off") continue;
      const variant = document.createElement("div");
      variant.className = "model-variant-pill provider-model-variant";

      const statusDot = document.createElement("span");
      statusDot.className = "connection-result success";

      const label = document.createElement("span");
      label.className = "model-variant-label";
      label.textContent = reasoningLevelLabel(level);

      variant.append(statusDot, label);
      container.append(variant);
    }
  }

  // 原始数据没有返回推理等级时，直接显示“默认”
  if (container.children.length === 0) {
    const variant = document.createElement("div");
    variant.className = "model-variant-pill provider-model-variant";

    const statusDot = document.createElement("span");
    statusDot.className = "connection-result success";

    const label = document.createElement("span");
    label.className = "model-variant-label";
    label.textContent = t("models.defaultVariant");

    variant.append(statusDot, label);
    container.append(variant);
  }

  return container;
}

function positiveMinimum(...values: Array<number | undefined>): number | null {
  const positiveValues = values.filter(
    (value): value is number => value !== undefined && Number.isFinite(value) && value > 0,
  );
  return positiveValues.length > 0 ? Math.min(...positiveValues) : null;
}

function capabilityBadge(type: "vision" | "tools" | "reasoning"): HTMLSpanElement {
  const icons = {
    vision: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
    tools: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`,
    reasoning: `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  };
  const labels = {
    vision: t("models.vision"),
    tools: t("models.tools"),
    reasoning: t("models.reasoning"),
  };
  const badge = document.createElement("span");
  badge.className = "capability-badge";
  badge.title = labels[type];
  badge.innerHTML = `${icons[type]}${labels[type]}`;
  return badge;
}

function createEndpoint(): HTMLElement {
  const endpoint = document.createElement("code");
  endpoint.className = "provider-endpoint";
  endpoint.title = OFFICIAL_ENDPOINT_URL;

  const text = document.createElement("span");
  text.className = "provider-endpoint-text";
  text.textContent = OFFICIAL_ENDPOINT_URL;

  const copyButton = document.createElement("button");
  copyButton.type = "button";
  copyButton.className = "copy-endpoint-btn";
  copyButton.title = t("models.copyEndpoint");
  copyButton.innerHTML = COPY_ICON;
  copyButton.addEventListener("click", () => {
    void navigator.clipboard.writeText(OFFICIAL_ENDPOINT_URL)
      .then(() => {
        copyButton.innerHTML = COPIED_ICON;
        window.setTimeout(() => { copyButton.innerHTML = COPY_ICON; }, 2000);
      })
      .catch(() => {
        showNotice(t("models.copyEndpoint"));
      });
  });

  endpoint.append(text, copyButton);
  return endpoint;
}

function createOfficialHeading(upstreamCount: number): HTMLDivElement {
  const heading = document.createElement("div");
  heading.className = "provider-card-heading";

  const identity = document.createElement("div");
  identity.className = "provider-identity";

  const title = document.createElement("h3");
  title.textContent = t("models.officialTitle");

  identity.append(title, createEndpoint());

  const metadata = document.createElement("div");
  metadata.className = "provider-meta";

  const protocol = document.createElement("span");
  protocol.className = "status-pill neutral";
  protocol.textContent = t("models.officialMetaTag");

  const count = document.createElement("strong");
  count.textContent = `${upstreamCount} ${t("models.upstreamModels")}`;

  metadata.append(protocol, count);
  heading.append(identity, metadata);
  return heading;
}

export function renderOfficialProviderCard(): { element: HTMLElement; dispose: () => void } {
  const card = document.createElement("article");
  card.className = "provider-card";

  // 1. Heading
  card.append(createOfficialHeading(MAIN_AGENT_MODEL_IDS.length));

  // 2. Actions 分割行
  const actions = document.createElement("div");
  actions.className = "provider-actions";



  const testActions = document.createElement("div");
  testActions.className = "provider-test-actions";

  const testBtn = document.createElement("button");
  testBtn.type = "button";
  testBtn.className = "secondary compact-button";
  testBtn.textContent = t("models.testConnection");

  const summary = document.createElement("span");
  summary.className = "provider-test-summary";
  summary.textContent = t("models.fetching");

  testBtn.addEventListener("click", () => {
    testBtn.disabled = true;
    testBtn.textContent = t("models.testing");
    const startTime = performance.now();
    void fetchOfficialModels()
      .then(() => {
        const duration = Math.round(performance.now() - startTime);
        summary.className = "provider-test-summary success";
        summary.textContent = t("models.testSuccess", { time: String(duration) });
      })
      .catch((err: unknown) => {
        summary.className = "provider-test-summary error";
        summary.textContent = t("models.testFailed", { msg: errorMessage(err) });
      })
      .finally(() => {
        testBtn.disabled = false;
        testBtn.textContent = t("models.testConnection");
      });
  });

  const refreshBtn = document.createElement("button");
  refreshBtn.type = "button";
  refreshBtn.className = "secondary compact-button";
  refreshBtn.textContent = t("overview.refresh");
  refreshBtn.addEventListener("click", () => {
    refreshBtn.disabled = true;
    summary.className = "provider-test-summary";
    summary.textContent = t("models.fetching");
    loadOfficialModels().finally(() => {
      refreshBtn.disabled = false;
    });
  });

  testActions.append(testBtn, refreshBtn, summary);
  actions.append(testActions);
  card.append(actions);

  // 3. Models 列表 (经典 3 列 Grid: 上游模型 | 能力 | 模型映射)
  const modelsContainer = document.createElement("div");
  modelsContainer.className = "provider-models";

  const loadingState = document.createElement("p");
  loadingState.className = "provider-model-empty";
  loadingState.textContent = t("models.officialFetching");
  modelsContainer.append(loadingState);

  card.append(modelsContainer);

  let isDisposed = false;

  const loadOfficialModels = () => {
    loadingState.textContent = t("models.officialFetching");
    modelsContainer.replaceChildren(loadingState);
    return fetchOfficialModels()
    .then((models) => {
      if (isDisposed) return;
      modelsContainer.replaceChildren();

      const mainModels = filterMainAgentModels(models);
      summary.className = "provider-test-summary success";
      summary.textContent = t("models.officialStatusOk", { count: mainModels.length });

      if (!mainModels || mainModels.length === 0) {
        const empty = document.createElement("p");
        empty.className = "provider-model-empty";
        empty.textContent = t("models.officialEmpty");
        modelsContainer.append(empty);
        return;
      }

      // 4 列 Grid 表头
      const header = document.createElement("div");
      header.className = "provider-models-header";
      for (const label of [
        t("models.upstreamModels"),
        t("models.capabilityColumn"),
        t("models.reasoningLevelColumn"),
        t("models.virtualModels"),
      ]) {
        const column = document.createElement("span");
        column.textContent = label;
        header.append(column);
      }
      modelsContainer.append(header);

      // 经典 3 列 Grid 模型行
      for (const item of mainModels) {
        const row = document.createElement("article");
        row.className = "provider-model-item";

        // 第一列：模型名称
        const main = document.createElement("div");
        main.className = "provider-model-main";
        const name = document.createElement("h4");
        name.textContent = item.displayName || item.id;
        main.append(name);

        const capObj = item.capabilities as Record<string, unknown> | undefined;
        if (capObj?.raw_config) {
          const rawButton = document.createElement("button");
          rawButton.type = "button";
          rawButton.className = "raw-config-button";
          rawButton.textContent = "{…}";
          rawButton.title = t("models.viewRawConfig");
          rawButton.setAttribute("aria-label", t("models.viewRawConfigForModel", {
            model: item.displayName || item.id,
          }));
          rawButton.addEventListener("click", () => {
            showRawConfigModal(item.displayName || item.id, capObj.raw_config);
          });

          const titleWrapper = document.createElement("div");
          titleWrapper.className = "provider-model-title-row";
          name.remove();
          titleWrapper.append(name, rawButton);
          main.append(titleWrapper);
        }

        // 第二列：能力 Badge
        const capabilities = document.createElement("div");
        capabilities.className = "capability-list";
        const caps = capObj as Record<string, boolean> | undefined;
        if (caps?.vision) capabilities.append(capabilityBadge("vision"));
        if (caps?.tools) capabilities.append(capabilityBadge("tools"));
        if (caps?.reasoning) capabilities.append(capabilityBadge("reasoning"));

        // 第三列：推理档位 (基于模型数据或默认)
        const variants = createReasoningVariantPills(item);

        // 第四列：压缩策略 (根据当前模型动态获取)
        const policyCol = document.createElement("div");
        policyCol.className = "provider-model-variants-inline";
        const capacity = positiveMinimum(item.inputTokenLimit, item.maxTokens, item.contextWindow);
        const outputTokenLimit = positiveMinimum(item.outputTokenLimit);
        const currentPolicy = store.config.model_compression_policies[item.id] ?? null;
        const status = getPolicyPillStatus(
          currentPolicy,
          capacity,
          outputTokenLimit,
          t("models.presetOfficialDefault"),
        );

        const policyButton = document.createElement("button");
        policyButton.type = "button";
        policyButton.className = `policy-pill status-pill ${status.isManaged ? "accent" : "neutral"}`;
        policyButton.dataset.policyFocusKey = `official:${item.id}`;
        policyButton.title = t("models.editPolicyTitle");
        policyButton.setAttribute("aria-label", t("models.editPolicyForModel", {
          model: item.displayName || item.id,
          status: status.label,
        }));
        const policyLabel = document.createElement("span");
        policyLabel.textContent = status.label;
        policyButton.append(policyLabel);
        policyButton.insertAdjacentHTML("beforeend", `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/></svg>`);
        policyButton.addEventListener("click", () => {
          showPolicyEditorModal({
            modelName: item.displayName || item.id,
            currentPolicy,
            capacity,
            outputTokenLimit,
            defaultLabel: t("models.presetOfficialDefault"),
            defaultHelp: t("models.policyOfficialDefaultHelp"),
            emptyNotice: t("models.policyEmptyNotice"),
            upstreamCompression: item.upstreamCompression,
            preferCurrentWorker: false,
            focusKey: `official:${item.id}`,
            onSave: async (policy) => {
              await updateConfig((current) => {
                const policies = { ...current.model_compression_policies };
                if (policy) policies[item.id] = policy;
                else delete policies[item.id];
                return { ...current, model_compression_policies: policies };
              });
            },
          });
        });
        policyCol.append(policyButton);

        row.append(main, capabilities, variants, policyCol);
        modelsContainer.append(row);
      }
    })
    .catch((err: unknown) => {
      if (isDisposed) return;
      const message = errorMessage(err);
      summary.className = "provider-test-summary error";
      summary.textContent = t("models.officialStatusFailed");
      modelsContainer.replaceChildren();
      const errorMsg = document.createElement("p");
      errorMsg.className = "provider-model-empty";
      errorMsg.textContent = t("models.officialFetchFailed", { message });
      modelsContainer.append(errorMsg);
    });
  };

  void loadOfficialModels();

  return {
    element: card,
    dispose: () => {
      isDisposed = true;
    },
  };
}

function filterMainAgentModels(models: ProviderCatalogModel[]): ProviderCatalogModel[] {
  const modelMap = new Map(models.map((m) => [m.id, m]));
  const result: ProviderCatalogModel[] = [];

  for (const id of MAIN_AGENT_MODEL_IDS) {
    const item = modelMap.get(id);
    if (item) {
      result.push(item);
    }
  }

  return result;
}
