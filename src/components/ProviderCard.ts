import type { Provider, UpstreamModel, VirtualModel } from "../types/config";
import type { ModelConnectionTestOutcome, ConnectionTestViewState } from "../types/proxy";
import { store } from "../store/appStore";
import {
  removeProvider as removeProviderCommand,
  testVirtualModelConnection as testVirtualModelConnectionCommand,
} from "../controllers/providerController";
import {
  connectionTestResults,
  connectionTestsInFlight,
  isProviderEditorDirty,
  providerTestSessions,
} from "../features/providers/providerState";
import { showNotice } from "./NoticeBar";
import { armDestructiveButton, withBusy, errorMessage } from "../utils/domUtils";
import { protocolName } from "../utils/modelUtils";
import { reasoningLevelLabel, sortVirtualModelsByReasoningLevel } from "../utils/reasoningUtils";
import { t } from "../i18n";

export async function removeProvider(
  providerId: string,
  button: HTMLButtonElement,
  onChanged: () => void,
): Promise<void> {
  await withBusy(button, async () => {
    await removeProviderCommand(providerId);
    onChanged();
    showNotice(t("models.providerDeleted"));
  }, t("models.deleting"));
}

export const connectionTestWaiters: Array<() => void> = [];
export let activeConnectionTests = 0;

function capabilityBadge(type: "vision" | "tools" | "reasoning"): HTMLSpanElement {
  const badge = document.createElement("span");
  badge.className = "capability-badge";
  let icon = "";
  let text = "";
  if (type === "vision") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`;
    text = t("models.vision") || "Vision";
  } else if (type === "tools") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`;
    text = t("models.tools") || "Tools";
  } else if (type === "reasoning") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`;
    text = t("models.reasoning") || "Thinking";
  }
  badge.title = text;
  badge.innerHTML = `${icon}${text}`;
  return badge;
}

function providerModelGroup(
  upstream: UpstreamModel,
  virtualModels: VirtualModel[],
): HTMLElement {
  const item = document.createElement("article");
  item.className = "provider-model-item";

  const main = document.createElement("div");
  main.className = "provider-model-main";
  const name = document.createElement("h4");
  name.textContent = upstream.display_name;
  main.append(name);

  const capabilities = document.createElement("div");
  capabilities.className = "capability-list";
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("vision"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("tools"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("reasoning"));
  }

  const variants = document.createElement("div");
  variants.className = "provider-model-variants-inline";
  const sortedVirtualModels = sortVirtualModelsByReasoningLevel(virtualModels);
  for (const virtualModel of sortedVirtualModels) {
    const variant = document.createElement("div");
    variant.className = "model-variant-pill provider-model-variant";
    variant.dataset.virtualModelId = virtualModel.id;
    variant.title = virtualModel.display_name;

    const label = document.createElement("span");
    label.className = "model-variant-label";
    label.textContent = virtualModel.default_reasoning_level
      ? reasoningLevelLabel(virtualModel.default_reasoning_level)
      : t("models.defaultVariant");

    const connectionResult = document.createElement("span");
    connectionResult.className = "connection-result";
    connectionResult.setAttribute("role", "status");
    connectionResult.setAttribute("aria-live", "polite");
    connectionResult.hidden = true;
    const existingState = connectionTestResults.get(virtualModel.id);
    if (existingState) renderConnectionTestState(connectionResult, existingState);

    variant.append(label, connectionResult);
    variants.append(variant);
  }

  item.append(main, capabilities, variants);
  return item;
}

async function withConnectionTestSlot<T>(action: () => Promise<T>): Promise<T> {
  if (activeConnectionTests < 3) {
    activeConnectionTests += 1;
  } else {
    await new Promise<void>((resolve) => connectionTestWaiters.push(resolve));
  }

  try {
    return await action();
  } finally {
    const next = connectionTestWaiters.shift();
    if (next) next();
    else activeConnectionTests -= 1;
  }
}

function sharedConnectionTest(virtualModelId: string): Promise<ModelConnectionTestOutcome> {
  const existingTest = connectionTestsInFlight.get(virtualModelId);
  if (existingTest) return existingTest;

  const test = withConnectionTestSlot(async () => {
    try {
      const result = await testVirtualModelConnectionCommand(virtualModelId);
      return { kind: "result", result } as const;
    } catch (error) {
      return { kind: "error", message: errorMessage(error) } as const;
    }
  });
  connectionTestsInFlight.set(virtualModelId, test);
  const clear = () => {
    if (connectionTestsInFlight.get(virtualModelId) === test) {
      connectionTestsInFlight.delete(virtualModelId);
    }
  };
  void test.then(clear, clear);
  return test;
}

function renderConnectionTestState(target: HTMLElement, state: ConnectionTestViewState): void {
  const message = state.status === "testing"
    ? t("models.testing")
    : state.status === "success"
      ? t("models.testSuccess", { time: state.durationMs })
      : t("models.testFailed", { msg: state.message });
  target.hidden = false;
  target.className = `connection-result ${state.status === "testing" ? "pending" : state.status}`;
  target.textContent = message;
  target.title = message;
}

async function testVirtualModelConnection(
  virtualModelId: string,
  target: HTMLElement,
): Promise<boolean> {
  const pending: ConnectionTestViewState = { status: "testing" };
  connectionTestResults.set(virtualModelId, pending);
  renderConnectionTestState(target, pending);

  const outcome = await sharedConnectionTest(virtualModelId);
  if (outcome.kind === "result") {
    const state: ConnectionTestViewState = outcome.result.success
      ? {
          status: "success",
          durationMs: outcome.result.durationMs,
        }
      : { status: "error", message: outcome.result.message };
    connectionTestResults.set(virtualModelId, state);
    renderConnectionTestState(target, state);
    return outcome.result.success;
  }

  const state: ConnectionTestViewState = {
    status: "error",
    message: outcome.message,
  };
  connectionTestResults.set(virtualModelId, state);
  renderConnectionTestState(target, state);
  return false;
}

async function testProviderModels(
  providerId: string,
  card: HTMLElement,
  virtualModels: VirtualModel[],
  sessionVirtualModelIds: string[],
  progressButton: HTMLButtonElement,
  onChanged: () => void,
): Promise<void> {
  const rows = [...card.querySelectorAll<HTMLElement>(".provider-model-variant")];
  const resultTargets = new Map(rows.map((row) => [
    row.dataset.virtualModelId,
    row.querySelector<HTMLElement>(".connection-result"),
  ]));
  let nextIndex = 0;
  let completed = 0;
  let succeeded = 0;
  const worker = async () => {
    while (nextIndex < virtualModels.length) {
      const virtualModel = virtualModels[nextIndex];
      nextIndex += 1;
      const target = resultTargets.get(virtualModel.id);
      if (target && await testVirtualModelConnection(virtualModel.id, target)) {
        succeeded += 1;
      }
      completed += 1;
      progressButton.textContent = t("models.testProgressSimple", {
        current: completed,
        total: virtualModels.length,
      });
    }
  };

  const concurrency = Math.min(3, virtualModels.length);
  await Promise.all(Array.from({ length: concurrency }, worker));

  const failed = virtualModels.length - succeeded;
  providerTestSessions.set(providerId, {
    targetVirtualModelIds: sessionVirtualModelIds,
    completedAt: Date.now(),
  });
  showNotice(
    t("models.testsSummary", { succeeded, failed }),
    failed > 0 ? "error" : "success",
  );
  window.setTimeout(onChanged, 0);
}

function fallbackRemovalBlocker(removedIds: Set<string>): string | null {
  const source = store.config?.virtual_models.find(
    (model) => !removedIds.has(model.id)
      && model.fallback_virtual_model_id
      && removedIds.has(model.fallback_virtual_model_id),
  );
  if (!source?.fallback_virtual_model_id) return null;
  const removed = store.config?.virtual_models.find(
    (model) => model.id === source.fallback_virtual_model_id,
  );
  return t("models.fallbackBlocker", {
    source: source.display_name,
    fallback: removed?.display_name ?? source.fallback_virtual_model_id,
  });
}

function destructiveMutationBlocker(removedIds: Set<string>): string | null {
  return fallbackRemovalBlocker(removedIds);
}

export interface ProviderCardActions {
  onEdit: () => void;
  onChanged: () => void;
}

export function renderSingleProviderCard(provider: Provider, actions: ProviderCardActions): HTMLElement {
  const card = document.createElement("article");
  card.className = "provider-card";
  const heading = document.createElement("div");
  heading.className = "provider-card-heading";
  const identity = document.createElement("div");
  identity.className = "provider-identity";
  const title = document.createElement("h3");
  title.textContent = provider.name;
  const protocol = document.createElement("span");
  protocol.className = "status-pill neutral";
  protocol.textContent = protocolName(provider.protocol);
  const endpointText = document.createElement("span");
  endpointText.className = "provider-endpoint-text";
  endpointText.textContent = provider.models_endpoint;

  const copyButton = document.createElement("button");
  copyButton.type = "button";
  copyButton.className = "copy-endpoint-btn";
  copyButton.title = t("models.copyEndpoint");
  copyButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>`;
  copyButton.addEventListener("click", () => {
    navigator.clipboard.writeText(provider.models_endpoint).then(() => {
      const originalHtml = copyButton.innerHTML;
      copyButton.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;
      setTimeout(() => { copyButton.innerHTML = originalHtml; }, 2000);
    });
  });

  const endpoint = document.createElement("code");
  endpoint.className = "provider-endpoint";
  endpoint.title = provider.models_endpoint;
  endpoint.append(endpointText, copyButton);
  identity.append(title, endpoint);

  const providerUpstreams = (store.config?.upstream_models || []).filter(
    (upstream) => upstream.provider_id === provider.id,
  );
  const modelLinks = (store.config?.virtual_models || []).flatMap((virtualModel) => {
    const upstream = providerUpstreams.find(
      (item) => item.id === virtualModel.upstream_model_id,
    );
    return upstream ? [{ virtualModel, upstream }] : [];
  });
  const providerMeta = document.createElement("div");
  providerMeta.className = "provider-meta";
  const count = document.createElement("strong");
  count.textContent = `${providerUpstreams.length} ${t("models.upstreamModels")}`;
  providerMeta.append(protocol, count);
  heading.append(identity, providerMeta);

  const providerActions = document.createElement("div");
  providerActions.className = "provider-actions";
  const providerEditActions = document.createElement("div");
  providerEditActions.className = "provider-edit-actions";
  const manage = document.createElement("button");
  manage.type = "button";
  manage.className = "secondary compact-button";
  manage.textContent = t("models.editProvider");
  manage.addEventListener("click", actions.onEdit);
  const removeProviderButton = document.createElement("button");
  removeProviderButton.type = "button";
  removeProviderButton.className = "danger-text";
  removeProviderButton.textContent = t("models.deleteProvider");
  
  armDestructiveButton(
    removeProviderButton,
    `${t("models.deleteProvider")} (${modelLinks.length})`,
    () => removeProvider(provider.id, removeProviderButton, actions.onChanged),
    () => {
      if (isProviderEditorDirty()) {
        return t("models.unsavedChangesBlocker");
      }
      return destructiveMutationBlocker(new Set(modelLinks.map(({ virtualModel }) => virtualModel.id)));
    },
  );
  providerEditActions.append(manage, removeProviderButton);

  const providerTestActions = document.createElement("div");
  providerTestActions.className = "provider-test-actions";
  const testAllModels = document.createElement("button");
  testAllModels.type = "button";
  testAllModels.className = "secondary compact-button provider-bulk-test";
  const allVirtualModels = modelLinks.map(({ virtualModel }) => virtualModel);
  const failedVirtualModels = allVirtualModels.filter(
    (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "error",
  );
  const currentVirtualIds = allVirtualModels.map((model) => model.id).sort();
  const storedTestSession = providerTestSessions.get(provider.id);
  const testSession = storedTestSession
    && JSON.stringify([...storedTestSession.targetVirtualModelIds].sort()) === JSON.stringify(currentVirtualIds)
    ? storedTestSession
    : undefined;
  testAllModels.textContent = testSession
    ? failedVirtualModels.length > 0
      ? t("models.retryFailed", { count: failedVirtualModels.length })
      : t("models.testConnection")
    : t("models.testConnection");
  testAllModels.disabled = modelLinks.length === 0;
  testAllModels.addEventListener("click", () => {
    const currentFailures = allVirtualModels.filter(
      (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "error",
    );
    const targets = testSession && currentFailures.length > 0
      ? currentFailures
      : allVirtualModels;
    void withBusy(
      testAllModels,
      () => testProviderModels(
        provider.id,
        card,
        targets,
        currentVirtualIds,
        testAllModels,
        actions.onChanged,
      ),
      t("models.testing"),
    );
  });
  const testSummary = document.createElement("span");
  testSummary.className = "provider-test-summary";
  if (testSession) {
    const passed = allVirtualModels.filter(
      (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "success",
    ).length;
    testSummary.classList.add(failedVirtualModels.length > 0 ? "error" : "success");
    testSummary.textContent = t("models.testsOk", { passed, total: allVirtualModels.length });
    providerTestActions.append(testSummary);
  }
  providerTestActions.append(testAllModels);
  providerActions.append(providerEditActions, providerTestActions);

  const models = document.createElement("div");
  models.className = "provider-models";
  if (modelLinks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = t("models.emptyTitle");
    models.append(empty);
  } else {
    const modelsHeader = document.createElement("div");
    modelsHeader.className = "provider-models-header";
    for (const label of [t("models.upstreamModels"), t("models.capabilityColumn"), t("models.virtualModels")]) {
      const column = document.createElement("span");
      column.textContent = label;
      modelsHeader.append(column);
    }
    models.append(modelsHeader);

    for (const upstream of providerUpstreams) {
      const virtualModels = modelLinks
        .filter((link) => link.upstream.id === upstream.id)
        .map((link) => link.virtualModel);
      if (virtualModels.length > 0) {
        models.append(providerModelGroup(upstream, virtualModels));
      }
    }
  }

  card.append(heading, providerActions, models);
  return card;
}
