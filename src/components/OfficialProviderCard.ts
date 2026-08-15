import { fetchOfficialModels } from "../controllers/providerController";
import { t } from "../i18n";
import { store } from "../store/appStore";
import type { ProviderCatalogModel } from "../types/catalog";
import { errorMessage } from "../utils/errorUtils";
import { createOfficialModelsDebugButtons } from "./OfficialModelsDebug";
import { buildOfficialModelCards } from "./official/officialModelCardBuilder";
import {
  filterMainAgentModels,
  isOfficialSourceUnavailable,
  officialModelAliases,
  synchronizeOfficialModelPolicies,
} from "./official/officialModelUtils";

let cachedOfficialModels: ProviderCatalogModel[] | null = null;
let cachedModelAliases: Map<string, string> = new Map();

export function getCachedOfficialModelCount(): number | null {
  if (!cachedOfficialModels) return null;
  const mainModels = filterMainAgentModels(cachedOfficialModels);
  const disabledSet = new Set(store.config.disabled_official_models);
  return mainModels.filter((model) => !disabledSet.has(model.id)).length;
}

export function renderOfficialProviderCard(options: {
  onModelCountChange?: (count: number | null) => void;
} = {}): { element: HTMLElement; dispose: () => void } {
  const card = document.createElement("article");
  card.className = "provider-card";

  // Card Top Toolbar (测试状态提示 + 操作按钮)
  const toolbar = document.createElement("div");
  toolbar.className = "provider-card-toolbar";

  const toolbarLeft = document.createElement("div");
  toolbarLeft.className = "provider-toolbar-left";

  const summary = document.createElement("span");
  summary.className = "provider-test-summary";
  toolbarLeft.append(summary);

  const toolbarRight = document.createElement("div");
  toolbarRight.className = "provider-toolbar-right";

  const testBtn = document.createElement("button");
  testBtn.type = "button";
  testBtn.className = "secondary compact-button";
  testBtn.textContent = t("models.testConnection");

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
    summary.textContent = "";
    loadOfficialModels(true).finally(() => {
      refreshBtn.disabled = false;
    });
  });

  toolbarRight.append(testBtn, refreshBtn);
  toolbarRight.append(...createOfficialModelsDebugButtons());
  toolbar.append(toolbarLeft, toolbarRight);
  card.append(toolbar);

  // Table Wrapper & Models 列表
  const tableWrapper = document.createElement("div");
  tableWrapper.className = "provider-table-wrapper";

  const modelsContainer = document.createElement("div");
  modelsContainer.className = "provider-models";

  const refreshCardList = () => {
    if (!cachedOfficialModels) return;
    options.onModelCountChange?.(getCachedOfficialModelCount());
    modelsContainer.replaceChildren(
      ...buildOfficialModelCards(cachedOfficialModels, cachedModelAliases, refreshCardList),
    );
  };

  // 1. 若已有缓存数据：立即同步渲染已有模型，杜绝空白与 Loading 闪烁
  if (cachedOfficialModels && cachedOfficialModels.length > 0) {
    refreshCardList();
  } else {
    // 首次进入无缓存时展示 Loading 提示
    const loadingState = document.createElement("p");
    loadingState.className = "provider-model-empty";
    loadingState.textContent = t("models.officialFetching");
    modelsContainer.append(loadingState);
  }

  tableWrapper.append(modelsContainer);
  card.append(tableWrapper);

  let isDisposed = false;

  const loadOfficialModels = (isManualRefresh = false) => {
    if (!cachedOfficialModels && !isManualRefresh) {
      options.onModelCountChange?.(null);
    }
    return fetchOfficialModels()
      .then((models) => {
        if (isDisposed) return;
        cachedOfficialModels = models;
        cachedModelAliases = officialModelAliases(models);

        void synchronizeOfficialModelPolicies(cachedModelAliases).catch((error: unknown) => {
          console.warn("同步官方模型策略失败，代理运行时仍会按目录映射兼容", error);
        });
        options.onModelCountChange?.(getCachedOfficialModelCount());
        summary.className = "provider-test-summary";
        summary.textContent = "";

        // 平滑就地替换卡片
        modelsContainer.replaceChildren(
          ...buildOfficialModelCards(models, cachedModelAliases, refreshCardList),
        );
      })
      .catch((err: unknown) => {
        if (isDisposed) return;
        const message = errorMessage(err);
        summary.className = "provider-test-summary error";
        summary.textContent = t("models.officialStatusFailed");

        if (isOfficialSourceUnavailable(err)) {
          cachedOfficialModels = null;
          cachedModelAliases = new Map();
        }
        if (!cachedOfficialModels || cachedOfficialModels.length === 0) {
          options.onModelCountChange?.(null);
          modelsContainer.replaceChildren();
          const errorMsg = document.createElement("p");
          errorMsg.className = "provider-model-empty";
          errorMsg.textContent = t("models.officialFetchFailed", { message });
          modelsContainer.append(errorMsg);
        }
      });
  };

  // 后台静默刷新（若已有缓存则无感知校验，若无缓存则拉取初始数据）
  void loadOfficialModels(false);

  return {
    element: card,
    dispose: () => {
      isDisposed = true;
    },
  };
}
