import { removeProvider as removeProviderCommand } from "../../controllers/providerController";
import {
  testProviderModels,
} from "../../features/providers/providerConnectionTests";
import {
  connectionTestResults,
  isProviderEditorDirty,
  providerTestSessions,
} from "../../features/providers/providerState";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type { Provider, VirtualModel } from "../../types/config";
import { armDestructiveButton, element, withBusy } from "../../utils/domUtils";
import { showNotice } from "../NoticeBar";
import type { ProviderModelLink } from "./ProviderCardModels";

export interface ProviderCardActions {
  onEdit: () => void;
  onChanged: () => void;
}

interface RenderedProviderActions {
  element: HTMLDivElement;
  dispose: () => void;
}

async function removeProvider(
  providerId: string,
  button: HTMLButtonElement,
): Promise<void> {
  const upstreamIds = new Set(
    store.config.upstream_models
      .filter((model) => model.provider_id === providerId)
      .map((model) => model.id),
  );
  const virtualModelIds = store.config.virtual_models
    .filter((model) => upstreamIds.has(model.upstream_model_id))
    .map((model) => model.id);
  const providerList = element<HTMLDivElement>("#provider-list");
  providerList.toggleAttribute("inert", true);
  try {
    await withBusy(button, async () => {
      await removeProviderCommand(providerId);
      for (const virtualModelId of virtualModelIds) connectionTestResults.delete(virtualModelId);
      providerTestSessions.delete(providerId);
      showNotice(t("models.providerDeleted"));
    }, showNotice, t("models.deleting"));
  } finally {
    providerList.removeAttribute("inert");
  }
}

function fallbackRemovalBlocker(removedIds: Set<string>): string | null {
  const source = store.config.virtual_models.find(
    (model) => !removedIds.has(model.id)
      && model.fallback_virtual_model_id
      && removedIds.has(model.fallback_virtual_model_id),
  );
  if (!source?.fallback_virtual_model_id) return null;
  const removed = store.config.virtual_models.find(
    (model) => model.id === source.fallback_virtual_model_id,
  );
  return t("models.fallbackBlocker", {
    source: source.display_name,
    fallback: removed?.display_name ?? source.fallback_virtual_model_id,
  });
}

function createEditActions(
  provider: Provider,
  modelLinks: ProviderModelLink[],
  actions: ProviderCardActions,
): { element: HTMLDivElement; dispose: () => void } {
  const container = document.createElement("div");
  container.className = "provider-edit-actions";
  const manage = document.createElement("button");
  manage.type = "button";
  manage.className = "secondary compact-button";
  manage.textContent = t("models.editProvider");
  manage.addEventListener("click", actions.onEdit);
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "danger-text";
  remove.dataset.i18n = "models.deleteProvider";
  remove.textContent = t("models.deleteProvider");
  const dispose = armDestructiveButton(
    remove,
    () => `${t("models.deleteProvider")} (${modelLinks.length})`,
    () => removeProvider(provider.id, remove),
    showNotice,
    () => isProviderEditorDirty()
      ? t("models.unsavedChangesBlocker")
      : fallbackRemovalBlocker(new Set(modelLinks.map(({ virtualModel }) => virtualModel.id))),
  );
  container.append(manage, remove);
  return { element: container, dispose };
}

function matchingTestSession(providerId: string, virtualModels: VirtualModel[]) {
  const currentIds = virtualModels.map((model) => model.id).sort();
  const session = providerTestSessions.get(providerId);
  if (!session) return { currentIds, session: undefined };
  const sessionIds = [...session.targetVirtualModelIds].sort();
  const matches = sessionIds.length === currentIds.length
    && sessionIds.every((id, index) => id === currentIds[index]);
  return { currentIds, session: matches ? session : undefined };
}

function createTestActions(
  provider: Provider,
  card: HTMLElement,
  modelLinks: ProviderModelLink[],
  onChanged: () => void,
): HTMLDivElement {
  const container = document.createElement("div");
  container.className = "provider-test-actions";
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary compact-button provider-bulk-test";
  const virtualModels = modelLinks.map(({ virtualModel }) => virtualModel);
  const failedModels = virtualModels.filter(
    (model) => connectionTestResults.get(model.id)?.status === "error",
  );
  const { currentIds, session } = matchingTestSession(provider.id, virtualModels);
  button.textContent = session && failedModels.length > 0
    ? t("models.retryFailed", { count: failedModels.length })
    : t("models.testConnection");
  button.disabled = virtualModels.length === 0;
  button.addEventListener("click", () => {
    const currentFailures = virtualModels.filter(
      (model) => connectionTestResults.get(model.id)?.status === "error",
    );
    const targets = session && currentFailures.length > 0 ? currentFailures : virtualModels;
    void withBusy(
      button,
      () => testProviderModels({
        providerId: provider.id,
        card,
        virtualModels: targets,
        sessionVirtualModelIds: currentIds,
        progressButton: button,
        notify: showNotice,
        onChanged,
      }),
      showNotice,
      t("models.testing"),
    );
  });
  if (session) {
    const passed = virtualModels.filter(
      (model) => connectionTestResults.get(model.id)?.status === "success",
    ).length;
    const summary = document.createElement("span");
    summary.className = "provider-test-summary";
    summary.classList.add(failedModels.length > 0 ? "error" : "success");
    summary.textContent = t("models.testsOk", { passed, total: virtualModels.length });
    container.append(summary);
  }
  container.append(button);
  return container;
}

export function createProviderCardActions(
  provider: Provider,
  card: HTMLElement,
  modelLinks: ProviderModelLink[],
  actions: ProviderCardActions,
): RenderedProviderActions {
  const container = document.createElement("div");
  container.className = "provider-actions";
  const editActions = createEditActions(provider, modelLinks, actions);
  container.append(
    editActions.element,
    createTestActions(provider, card, modelLinks, actions.onChanged),
  );
  return { element: container, dispose: editActions.dispose };
}
