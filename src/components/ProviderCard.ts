import { store } from "../store/appStore";
import type { Provider } from "../types/config";
import { createProviderCardActions, type ProviderCardActions } from "./providerCard/ProviderCardActions";
import {
  createProviderModels,
  type ProviderModelLink,
} from "./providerCard/ProviderCardModels";

interface RenderedProviderCard {
  element: HTMLElement;
  dispose: () => void;
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
    cardActions.element,
    createProviderModels(
      models.upstreams,
      models.links,
      provider.id,
      actions.onEditModel,
      actions.onChanged,
    ),
  );
  return { element: card, dispose: cardActions.dispose };
}
