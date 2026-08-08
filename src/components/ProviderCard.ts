import { store } from "../store/appStore";
import type { Provider } from "../types/config";
import { errorMessage } from "../utils/errorUtils";
import { protocolName } from "../utils/modelUtils";
import { createProviderCardActions, type ProviderCardActions } from "./providerCard/ProviderCardActions";
import {
  createProviderModels,
  type ProviderModelLink,
} from "./providerCard/ProviderCardModels";
import { showNotice } from "./NoticeBar";
import { t } from "../i18n";

const COPY_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
const COPIED_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;

interface RenderedProviderCard {
  element: HTMLElement;
  dispose: () => void;
}

function createEndpoint(provider: Provider): HTMLElement {
  const endpoint = document.createElement("code");
  endpoint.className = "provider-endpoint";
  endpoint.title = provider.models_endpoint;
  const text = document.createElement("span");
  text.className = "provider-endpoint-text";
  text.textContent = provider.models_endpoint;
  const copyButton = document.createElement("button");
  copyButton.type = "button";
  copyButton.className = "copy-endpoint-btn";
  copyButton.title = t("models.copyEndpoint");
  copyButton.innerHTML = COPY_ICON;
  copyButton.addEventListener("click", () => {
    void navigator.clipboard.writeText(provider.models_endpoint)
      .then(() => {
        copyButton.innerHTML = COPIED_ICON;
        window.setTimeout(() => { copyButton.innerHTML = COPY_ICON; }, 2000);
      })
      .catch((error) => {
        showNotice(t("overview.copyFailed", { message: errorMessage(error) }), "error");
      });
  });
  endpoint.append(text, copyButton);
  return endpoint;
}

function createProviderHeading(provider: Provider, upstreamCount: number): HTMLDivElement {
  const heading = document.createElement("div");
  heading.className = "provider-card-heading";
  const identity = document.createElement("div");
  identity.className = "provider-identity";
  const title = document.createElement("h3");
  title.textContent = provider.name;
  identity.append(title, createEndpoint(provider));
  const metadata = document.createElement("div");
  metadata.className = "provider-meta";
  const protocol = document.createElement("span");
  protocol.className = "status-pill neutral";
  protocol.textContent = protocolName(provider.protocol);
  const count = document.createElement("strong");
  count.textContent = `${upstreamCount} ${t("models.upstreamModels")}`;
  metadata.append(protocol, count);
  heading.append(identity, metadata);
  return heading;
}

function providerModelLinks(providerId: string): {
  upstreams: typeof store.config.upstream_models;
  links: ProviderModelLink[];
} {
  const upstreams = store.config.upstream_models.filter((upstream) => upstream.provider_id === providerId);
  const upstreamById = new Map(upstreams.map((upstream) => [upstream.id, upstream]));
  const links = store.config.virtual_models.flatMap((virtualModel) => {
    const upstream = upstreamById.get(virtualModel.upstream_model_id);
    return upstream ? [{ virtualModel, upstream }] : [];
  });
  return { upstreams, links };
}

export function renderSingleProviderCard(
  provider: Provider,
  actions: ProviderCardActions,
): RenderedProviderCard {
  const card = document.createElement("article");
  card.className = "provider-card";
  const models = providerModelLinks(provider.id);
  const cardActions = createProviderCardActions(provider, card, models.links, actions);
  card.append(
    createProviderHeading(provider, models.upstreams.length),
    cardActions.element,
    createProviderModels(models.upstreams, models.links),
  );
  return { element: card, dispose: cardActions.dispose };
}
