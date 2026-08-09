import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelTokenLimits } from "../../types/config";
import { resolveCatalogTokenLimits } from "./modelTokenLimits";

export function tokenLimitsFromCatalog(
  model: ProviderCatalogModel,
  existing: ModelTokenLimits | undefined,
  selected: ModelTokenLimits | undefined,
): ModelTokenLimits {
  return resolveCatalogTokenLimits(model, selected ?? existing);
}
