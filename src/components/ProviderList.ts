import { configService } from "../services/configService";
import { store } from "../store/appStore";
import { element, withBusy } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import {
  getActiveProviderTabId,
  setActiveProviderTabId,
} from "../features/providers/providerState";
import {
  OFFICIAL_PROVIDER,
  OFFICIAL_PROVIDER_ID,
} from "../features/providers/officialProvider";
import { t } from "../i18n";
import { showNotice } from "./NoticeBar";
import { renderSingleProviderCard } from "./ProviderCard";
import { renderOfficialProviderCard } from "./OfficialProviderCard";
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
  if (activeId === OFFICIAL_PROVIDER_ID) return OFFICIAL_PROVIDER_ID;
  if (activeId && providers.some((provider) => provider.id === activeId)) return activeId;
  setActiveProviderTabId(OFFICIAL_PROVIDER_ID);
  return OFFICIAL_PROVIDER_ID;
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

function createProviderTabs(activeId: string): DocumentFragment {
  const fragment = document.createDocumentFragment();
  const modelCounts = providerVirtualModelCounts();
  const allProviders = [OFFICIAL_PROVIDER, ...store.config.providers];

  for (const provider of allProviders) {
    const isOfficial = provider.id === OFFICIAL_PROVIDER_ID;
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = `provider-tab-card${provider.id === activeId ? " active" : ""}${
      isOfficial ? " official-tab" : ""
    }`;

    const icon = document.createElement("span");
    icon.className = "provider-tab-icon";
    if (isOfficial) {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>`;
    } else {
      icon.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="8" rx="2"/><rect x="2" y="14" width="20" height="8" rx="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></svg>`;
    }

    const title = document.createElement("span");
    title.className = "provider-tab-title";
    title.textContent = isOfficial ? t("models.officialTabName") : provider.name;

    const badge = document.createElement("span");
    badge.className = "provider-tab-badge";
    badge.textContent = isOfficial ? (cachedOfficialModelCount !== null ? String(cachedOfficialModelCount) : "—") : String(modelCounts.get(provider.id) ?? 0);

    tab.append(icon, title, badge);
    tab.addEventListener("click", () => {
      if (getActiveProviderTabId() === provider.id) return;
      setActiveProviderTabId(provider.id);
      renderProviders();
    });
    fragment.append(tab);
  }
  return fragment;
}

let cachedOfficialModelCount: number | null = null;

export function renderProviders(): void {
  const providerCount = document.querySelector<HTMLSpanElement>("#provider-count");
  const providerTabsBar = document.querySelector<HTMLDivElement>("#provider-tabs-bar");
  const providerList = element<HTMLDivElement>("#provider-list");
  element<HTMLButtonElement>("#open-provider-form").disabled = !store.configLoaded;
  disposeActiveProviderCard?.();
  disposeActiveProviderCard = null;
  providerList.replaceChildren();

  if (!store.configLoaded) {
    if (providerCount) providerCount.textContent = "—";
    providerTabsBar?.replaceChildren();
    renderConfigUnavailable(providerList);
    return;
  }

  const providers = store.config.providers;
  if (providerCount) providerCount.textContent = String(providers.length);
  const activeId = activeProviderId();

  // 将服务商 Tab 切换轨挂载在顶部同行容器中
  providerTabsBar?.replaceChildren(createProviderTabs(activeId));

  if (activeId === OFFICIAL_PROVIDER_ID) {
    const officialTabBadge = providerTabsBar?.querySelector<HTMLSpanElement>(
      ".official-tab .provider-tab-badge",
    );
    const activeCard = renderOfficialProviderCard({
      onModelCountChange: (count) => {
        cachedOfficialModelCount = count;
        if (officialTabBadge) officialTabBadge.textContent = count === null ? "—" : String(count);
      },
    });
    disposeActiveProviderCard = activeCard.dispose;
    providerList.append(activeCard.element);
    return;
  }

  const activeProvider =
    providers.find((provider) => provider.id === activeId) ?? providers[0];
  if (!activeProvider) {
    renderEmptyProviders(providerList);
    return;
  }

  const activeCard = renderSingleProviderCard(activeProvider, {
    onEdit: () => void openProviderEditor(activeProvider.id),
    onChanged: renderProviders,
  });
  disposeActiveProviderCard = activeCard.dispose;
  providerList.append(activeCard.element);
}

function renderEmptyProviders(container: HTMLDivElement): void {
  setActiveProviderTabId(OFFICIAL_PROVIDER_ID);
  const empty = document.createElement("p");
  empty.className = "empty-state";
  empty.textContent = t("models.emptyDesc");
  container.append(empty);
}
