import { element } from "../utils/domUtils";
import { store } from "../store/appStore";
import { renderReadiness } from "./ReadinessPanel";
import { renderSingleProviderCard } from "./ProviderCard";

export let activeProviderTabId: string | null = null;

export function setProviderEditorActiveTabId(id: string): void {
  activeProviderTabId = id;
}

export function renderProviders(): void {
  const providerCount = element<HTMLSpanElement>("#provider-count");
  const providerList = element<HTMLDivElement>("#provider-list");

  const providers = store.config?.providers ?? [];
  const upstreamModels = store.config?.upstream_models ?? [];
  const virtualModels = store.config?.virtual_models ?? [];

  providerCount.textContent = `${providers.length} 个服务`;
  providerList.replaceChildren();
  renderReadiness();

  if (providers.length === 0) {
    activeProviderTabId = null;
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = "还没有上游服务。添加连接后即可获取并选择模型。";
    providerList.append(empty);
    return;
  }

  if (!activeProviderTabId || !providers.some((p) => p.id === activeProviderTabId)) {
    activeProviderTabId = providers[0].id;
  }

  const tabsBar = document.createElement("div");
  tabsBar.className = "provider-tabs-bar";

  for (const provider of providers) {
    const tabCard = document.createElement("button");
    tabCard.type = "button";
    const isActive = provider.id === activeProviderTabId;
    tabCard.className = `provider-tab-card${isActive ? " active" : ""}`;

    const providerUpstreams = upstreamModels.filter(
      (upstream) => upstream.provider_id === provider.id,
    );
    const modelLinksCount = virtualModels.filter((virtualModel) => {
      return providerUpstreams.some((u) => u.id === virtualModel.upstream_model_id);
    }).length;

    const icon = document.createElement("span");
    icon.className = "provider-tab-icon";
    icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>`;

    const title = document.createElement("span");
    title.className = "provider-tab-title";
    title.textContent = provider.name;

    const badge = document.createElement("span");
    badge.className = "provider-tab-badge";
    badge.textContent = `${modelLinksCount}`;

    tabCard.append(icon, title, badge);
    tabCard.addEventListener("click", () => {
      if (activeProviderTabId !== provider.id) {
        activeProviderTabId = provider.id;
        renderProviders();
      }
    });
    tabsBar.append(tabCard);
  }

  providerList.append(tabsBar);

  const activeProvider = providers.find((p) => p.id === activeProviderTabId) ?? providers[0];
  const activeCard = renderSingleProviderCard(activeProvider);
  providerList.append(activeCard);
}
