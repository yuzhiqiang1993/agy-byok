import { element, errorMessage, withBusy } from "../utils/domUtils";
import { store } from "../store/appStore";
import { renderSingleProviderCard } from "./ProviderCard";
import { openProviderEditor } from "./ProviderEditor";
import { getActiveProviderTabId, setActiveProviderTabId } from "../features/providers/providerState";
import { t } from "../i18n";
import { configService } from "../services/configService";

export function renderProviders(): void {
  const providerCount = element<HTMLSpanElement>("#provider-count");
  const providerList = element<HTMLDivElement>("#provider-list");
  const openProviderFormButton = element<HTMLButtonElement>("#open-provider-form");

  openProviderFormButton.disabled = !store.configLoaded;
  providerList.replaceChildren();
  if (!store.configLoaded) {
    providerCount.textContent = "—";
    setActiveProviderTabId(null);
    const state = document.createElement("p");
    state.className = store.configLoadError ? "empty-state error-state" : "empty-state";
    state.textContent = store.configLoadError
      ? `${t("overview.loadFailed")}: ${store.configLoadError}`
      : t("overview.checking");
    providerList.append(state);
    if (store.configLoadError) {
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
        });
      });
      providerList.append(retryButton);
    }
    return;
  }

  const providers = store.config.providers;
  const upstreamModels = store.config.upstream_models;
  const virtualModels = store.config.virtual_models;

  providerCount.textContent = `${providers.length}`;

  if (providers.length === 0) {
    setActiveProviderTabId(null);
    const empty = document.createElement("p");
    empty.className = "empty-state";
    empty.textContent = t("models.emptyDesc");
    providerList.append(empty);
    return;
  }

  let activeProviderTabId = getActiveProviderTabId();
  if (!activeProviderTabId || !providers.some((p) => p.id === activeProviderTabId)) {
    activeProviderTabId = providers[0].id;
    setActiveProviderTabId(activeProviderTabId);
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
      if (getActiveProviderTabId() !== provider.id) {
        setActiveProviderTabId(provider.id);
        renderProviders();
      }
    });
    tabsBar.append(tabCard);
  }

  providerList.append(tabsBar);

  const activeProvider = providers.find((p) => p.id === activeProviderTabId) ?? providers[0];
  const activeCard = renderSingleProviderCard(activeProvider, {
    onEdit: () => void openProviderEditor(activeProvider.id),
    onChanged: renderProviders,
  });
  providerList.append(activeCard);
}
