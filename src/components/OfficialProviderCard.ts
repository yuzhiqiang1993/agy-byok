import { fetchOfficialModels } from "../controllers/providerController";
import type { ProviderCatalogModel } from "../types/catalog";
import { t } from "../i18n";

import { showNotice } from "./NoticeBar";
import { store } from "../store/appStore";
import { reasoningLevelLabel } from "../utils/reasoningUtils";

const COPY_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
const COPIED_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;

const OFFICIAL_ENDPOINT_URL = "https://daily-cloudcode-pa.googleapis.com";

// 官方 IDE 下拉菜单里 100% 对应的 11 个主力 Agent 模型白名单
const MAIN_AGENT_MODEL_IDS = [
  "gemini-3.6-flash-high",
  "gemini-3.6-flash-medium",
  "gemini-3.6-flash-low",
  "gemini-3-flash-agent",
  "gemini-3.5-flash-low",
  "gemini-3.5-flash-extra-low",
  "gemini-pro-agent",
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

function getOfficialModelManagedStatus(modelId: string): { label: string; isManaged: boolean } {
  const settings = store.config.official_model_settings;
  if (!settings) {
    return { label: t("models.officialStatusDirect"), isManaged: false };
  }

  const overridePolicy = settings.model_checkpoint_policies?.[modelId];
  if (overridePolicy && overridePolicy.enabled) {
    return { label: t("models.officialStatusManaged", { percent: "100" }), isManaged: true };
  }

  let activePolicy = null;
  const idLower = modelId.toLowerCase();
  if (idLower.includes("gemini")) {
    activePolicy = settings.gemini;
  } else if (idLower.includes("claude")) {
    activePolicy = settings.claude;
  }

  if (activePolicy && activePolicy.enabled) {
    const percent = activePolicy.mode === "percentage"
      ? activePolicy.token_threshold_percent
      : Math.round((activePolicy.token_threshold / 1000000) * 100);
    return { label: t("models.officialStatusManaged", { percent: String(percent) }), isManaged: true };
  }

  return { label: t("models.officialStatusDirect"), isManaged: false };
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
  summary.className = "provider-test-summary success";
  summary.textContent = t("models.officialStatusOk");

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
        summary.textContent = t("models.testFailed", { msg: String(err) });
      })
      .finally(() => {
        testBtn.disabled = false;
        testBtn.textContent = t("models.testConnection");
      });
  });

  testActions.append(testBtn, summary);
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

  const unsubscribeStore = store.subscribeConfig(() => {
    // 监听 Config，原先用来更新 Header Pill，现 Pill 已被移除
  });

  void fetchOfficialModels()
    .then((models) => {
      if (isDisposed) return;
      modelsContainer.replaceChildren();

      const mainModels = filterMainAgentModels(models);

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

        // 第二列：能力 Badge
        const capabilities = document.createElement("div");
        capabilities.className = "capability-list";
        const capObj = item.capabilities as Record<string, boolean> | undefined;
        if (capObj?.vision) capabilities.append(capabilityBadge("vision"));
        if (capObj?.tools) capabilities.append(capabilityBadge("tools"));
        if (capObj?.reasoning) capabilities.append(capabilityBadge("reasoning"));

        // 第三列：推理档位 (基于模型数据或默认)
        const variants = createReasoningVariantPills(item);

        // 第四列：压缩策略 (根据当前模型动态获取)
        const policyCol = document.createElement("div");
        policyCol.className = "provider-model-variants-inline";
        const curStatus = getOfficialModelManagedStatus(item.id);
        const policyPill = document.createElement("span");
        policyPill.className = curStatus.isManaged ? "status-pill active" : "status-pill neutral";
        policyPill.textContent = curStatus.label;
        policyCol.append(policyPill);

        row.append(main, capabilities, variants, policyCol);
        modelsContainer.append(row);
      }
    })
    .catch((err: unknown) => {
      if (isDisposed) return;
      modelsContainer.replaceChildren();
      const errorMsg = document.createElement("p");
      errorMsg.className = "provider-model-empty";
      errorMsg.textContent = `${t("models.officialFetchFailed")}: ${String(err)}`;
      modelsContainer.append(errorMsg);
    });

  return {
    element: card,
    dispose: () => {
      isDisposed = true;
      unsubscribeStore();
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
