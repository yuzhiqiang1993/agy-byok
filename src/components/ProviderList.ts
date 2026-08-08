import { configService } from "../services/configService";
import { store } from "../store/appStore";
import { element, withBusy } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import {
  getActiveProviderTabId,
  setActiveProviderTabId,
} from "../features/providers/providerState";
import { t } from "../i18n";
import { showNotice } from "./NoticeBar";
import { renderSingleProviderCard } from "./ProviderCard";
import { openProviderEditor } from "./ProviderEditor";

let disposeActiveProviderCard: (() => void) | null = null;

function renderConfigUnavailable(container: HTMLDivElement): void {
  setActiveProviderTabId(null);
  const state = document.createElement("p");
  state.className = store.configLoadError ? "empty-state error-state" : "empty-state";
  state.textContent = store.configLoadError
    ? `${t("overview.loadFailed")}: ${store.configLoadError}`
    : t("overview.checking");
  container.append(state);
  if (!store.configLoadError) return;
  const retryButton = document.createElement("button");
  retryButton.type = "button";
  retryButton.className = "secondary compact-button";
  retryButton.textContent = t("overview.refresh");
  retryButton.addEventListener("click", () => {
    void withBusy(retryButton, async () => {
      try {
        store.setConfig(await configService.getConfig());
      } catch (error) {
        store.setConfigFailed(errorMessage(error));
      }
    }, showNotice);
  });
  container.append(retryButton);
}

function activeProviderId(): string {
  const providers = store.config.providers;
  const activeId = getActiveProviderTabId();
  if (activeId && providers.some((provider) => provider.id === activeId)) return activeId;
  const firstId = providers[0].id;
  setActiveProviderTabId(firstId);
  return firstId;
}

function providerVirtualModelCounts(): Map<string, number> {
  const providerByUpstreamId = new Map(
    store.config.upstream_models.map((upstream) => [upstream.id, upstream.provider_id]),
  );
  const counts = new Map<string, number>();
  for (const virtualModel of store.config.virtual_models) {
    const providerId = providerByUpstreamId.get(virtualModel.upstream_model_id);
    if (providerId) counts.set(providerId, (counts.get(providerId) ?? 0) + 1);
  }
  return counts;
}

function createProviderTabs(activeId: string): HTMLDivElement {
  const tabs = document.createElement("div");
  tabs.className = "provider-tabs-bar";
  const modelCounts = providerVirtualModelCounts();
  for (const provider of store.config.providers) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = `provider-tab-card${provider.id === activeId ? " active" : ""}`;
    const icon = document.createElement("span");
    icon.className = "provider-tab-icon";
    icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>`;
    const title = document.createElement("span");
    title.className = "provider-tab-title";
    title.textContent = provider.name;
    const badge = document.createElement("span");
    badge.className = "provider-tab-badge";
    badge.textContent = String(modelCounts.get(provider.id) ?? 0);
    tab.append(icon, title, badge);
    tab.addEventListener("click", () => {
      if (getActiveProviderTabId() === provider.id) return;
      setActiveProviderTabId(provider.id);
      renderProviders();
    });
    tabs.append(tab);
  }
  return tabs;
}

function renderEmptyProviders(container: HTMLDivElement): void {
  setActiveProviderTabId(null);
  const empty = document.createElement("p");
  empty.className = "empty-state";
  empty.textContent = t("models.emptyDesc");
  container.append(empty);
}

export function renderProviders(): void {
  const providerCount = element<HTMLSpanElement>("#provider-count");
  const providerList = element<HTMLDivElement>("#provider-list");
  element<HTMLButtonElement>("#open-provider-form").disabled = !store.configLoaded;
  disposeActiveProviderCard?.();
  disposeActiveProviderCard = null;
  providerList.replaceChildren();
  if (!store.configLoaded) {
    providerCount.textContent = "—";
    renderConfigUnavailable(providerList);
    return;
  }

  const providers = store.config.providers;
  providerCount.textContent = String(providers.length);
  if (providers.length === 0) {
    renderEmptyProviders(providerList);
    return;
  }
  const activeId = activeProviderId();
  providerList.append(createProviderTabs(activeId));
  const activeProvider = providers.find((provider) => provider.id === activeId) ?? providers[0];
  const activeCard = renderSingleProviderCard(activeProvider, {
    onEdit: () => void openProviderEditor(activeProvider.id),
    onChanged: renderProviders,
  });
  disposeActiveProviderCard = activeCard.dispose;
  providerList.append(activeCard.element);
}
