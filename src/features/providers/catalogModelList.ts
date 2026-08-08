import { t } from "../../i18n";
import { element } from "../../utils/domUtils";
import { createCatalogModelRow } from "./catalogModelRow";
import type {
  CatalogModelListState,
  ProviderCatalogContext,
} from "./providerCatalogTypes";

export function renderCatalogModelList(
  context: ProviderCatalogContext,
  state: CatalogModelListState,
  onSelectionChange: () => void,
): void {
  const catalogModelList = element<HTMLDivElement>("#catalog-model-list");
  const query = element<HTMLInputElement>("#catalog-search").value.trim().toLowerCase();
  const visibleModels = state.catalogModels.filter((model) =>
    `${model.displayName} ${model.id}`.toLowerCase().includes(query)
  );
  const rerender = () => renderCatalogModelList(context, state, onSelectionChange);

  catalogModelList.replaceChildren(
    ...visibleModels.map((model) => createCatalogModelRow(model, context, state, rerender)),
  );

  if (visibleModels.length === 0) {
    const empty = document.createElement("p");
    empty.className = "empty-state compact-empty";
    empty.textContent = t("models.noMatchingModels");
    catalogModelList.append(empty);
  }

  onSelectionChange();
}
