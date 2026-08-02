import { invoke } from "@tauri-apps/api/core";
import type { Provider, UpstreamModel, VirtualModel } from "../types/config";
import type { ModelConnectionTestOutcome, ModelConnectionTestResult, ConnectionTestViewState } from "../types/proxy";
import { store } from "../store/appStore";
import { showNotice } from "./NoticeBar";
import { withBusy, errorMessage } from "../utils/domUtils";
import { formatActivityTime } from "../utils/displayUtils";
import { protocolName } from "../utils/modelUtils";
import { reasoningLevelLabel, sortVirtualModelsByReasoningLevel } from "../utils/reasoningUtils";
import { isProviderEditorDirty } from "./ProviderEditor";
import { openProviderEditor } from "./ProviderEditor";
import { renderProviders } from "./ProviderList";
import { configService } from "../services/configService";
import type { AppConfig } from "../types/config";

export async function persistConfig(nextConfig: AppConfig): Promise<void> {
  const result = await configService.saveConfig(nextConfig);
  store.setConfig(result);
  renderProviders();
}

export async function removeProvider(providerId: string, button: HTMLButtonElement): Promise<void> {
  void withBusy(button, async () => {
    if (!store.config) return;
    const nextConfig = JSON.parse(JSON.stringify(store.config));
    nextConfig.providers = nextConfig.providers.filter((p: any) => p.id !== providerId);
    nextConfig.upstream_models = nextConfig.upstream_models.filter((m: any) => m.provider_id !== providerId);
    const retainedUpstreamIds = new Set(nextConfig.upstream_models.map((m: any) => m.id));
    nextConfig.virtual_models = nextConfig.virtual_models.filter((m: any) => retainedUpstreamIds.has(m.upstream_model_id));
    await persistConfig(nextConfig);
    showNotice("上游服务及其关联模型已删除");
  }, "正在删除…");
}

export const connectionTestsInFlight = new Map<string, Promise<ModelConnectionTestOutcome>>();
export const connectionTestResults = new Map<string, ConnectionTestViewState>();
export const providerTestSessions = new Map<string, { targetVirtualModelIds: string[]; completedAt: number; }>();
export const connectionTestWaiters: Array<() => void> = [];
export let activeConnectionTests = 0;

function capabilityBadge(label: string): HTMLSpanElement {
  const badge = document.createElement("span");
  badge.className = "capability-badge";
  badge.title = label;
  let icon = "";
  if (label === "图像输入") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`;
  } else if (label === "工具调用") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></svg>`;
  } else if (label === "思考档位") {
    icon = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`;
  }
  const shortLabels: Record<string, string> = {
    图像输入: "图像",
    工具调用: "工具",
    思考档位: "思考",
  };
  badge.innerHTML = `${icon}${shortLabels[label] ?? label}`;
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
  if (upstream.capabilities.vision) capabilities.append(capabilityBadge("图像输入"));
  if (upstream.capabilities.tools) capabilities.append(capabilityBadge("工具调用"));
  if (Object.keys(upstream.capabilities.reasoning.levels).length > 0) {
    capabilities.append(capabilityBadge("思考档位"));
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
      : "Default";

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
      const result = await invoke<ModelConnectionTestResult>("test_model_connection", {
        virtualModelId,
      });
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
  target.hidden = false;
  target.className = `connection-result ${state.status === "testing" ? "pending" : state.status}`;
  target.textContent = state.message;
  target.title = state.message;
}

async function testVirtualModelConnection(
  virtualModelId: string,
  target: HTMLElement,
): Promise<boolean> {
  const pending: ConnectionTestViewState = { status: "testing", message: "测试中…" };
  connectionTestResults.set(virtualModelId, pending);
  renderConnectionTestState(target, pending);

  const outcome = await sharedConnectionTest(virtualModelId);
  if (outcome.kind === "result") {
    const state: ConnectionTestViewState = outcome.result.success
      ? {
          status: "success",
          message: `测试通过 · ${outcome.result.durationMs} ms`,
          durationMs: outcome.result.durationMs,
        }
      : { status: "error", message: `测试失败 · ${outcome.result.message}` };
    connectionTestResults.set(virtualModelId, state);
    renderConnectionTestState(target, state);
    return outcome.result.success;
  }

  const state: ConnectionTestViewState = {
    status: "error",
    message: `测试失败 · ${outcome.message}`,
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
      progressButton.textContent = `测试 ${completed}/${virtualModels.length}`;
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
    `测试完成：${succeeded} 个通过，${failed} 个失败`,
    failed > 0 ? "error" : "success",
  );
  window.setTimeout(renderProviders, 0);
}

export function armDestructiveButton(
  button: HTMLButtonElement,
  confirmLabel: string,
  action: () => Promise<void>,
  beforeArm?: () => string | null,
): void {
  const initialLabel = button.textContent ?? "删除";
  let armed = false;
  let resetTimer: number | null = null;
  const reset = () => {
    armed = false;
    if (resetTimer !== null) window.clearTimeout(resetTimer);
    resetTimer = null;
    button.textContent = initialLabel;
    button.classList.remove("danger-confirm");
  };
  button.addEventListener("click", () => {
    if (!armed) {
      const blocker = beforeArm?.();
      if (blocker) {
        showNotice(blocker, "error");
        return;
      }
      armed = true;
      button.textContent = confirmLabel;
      button.classList.add("danger-confirm");
      resetTimer = window.setTimeout(reset, 4000);
      return;
    }
    const blocker = beforeArm?.();
    if (blocker) {
      reset();
      showNotice(blocker, "error");
      return;
    }
    void action().finally(reset);
  });
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
  return `无法删除：模型入口“${source.display_name}”仍将“${removed?.display_name ?? source.fallback_virtual_model_id}”用作备用模型。请先调整 fallback。`;
}

function destructiveMutationBlocker(removedIds: Set<string>): string | null {
  // To avoid circular dependency with ProviderEditor, we can read a dirty flag or just return fallback blocker for now.
  // Actually, we can just check if dirty by using a getter from appStore or similar.
  // We'll assume the caller can pass it, or we rely on fallbackRemovalBlocker.
  // Let's keep it simple and just do fallback check here, if dirty, we check that in the callback.
  return fallbackRemovalBlocker(removedIds);
}

export function renderSingleProviderCard(provider: Provider): HTMLElement {
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
  copyButton.title = "复制接口地址";
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
  count.textContent = `${providerUpstreams.length} 个上游模型`;
  providerMeta.append(protocol, count);
  heading.append(identity, providerMeta);

  const providerActions = document.createElement("div");
  providerActions.className = "provider-actions";
  const providerEditActions = document.createElement("div");
  providerEditActions.className = "provider-edit-actions";
  const manage = document.createElement("button");
  manage.type = "button";
  manage.className = "secondary compact-button";
  manage.textContent = "编辑上游服务";
  manage.addEventListener("click", () => openProviderEditor(provider.id));
  const removeProviderButton = document.createElement("button");
  removeProviderButton.type = "button";
  removeProviderButton.className = "danger-text";
  removeProviderButton.textContent = "删除上游服务";
  
  armDestructiveButton(
    removeProviderButton,
    `确认删除及 ${modelLinks.length} 个入口`,
    () => removeProvider(provider.id, removeProviderButton),
    () => {
      if (isProviderEditorDirty()) {
        return "当前有未保存的上游服务修改，请先保存或取消编辑";
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
      ? `重试失败（${failedVirtualModels.length}）`
      : "重新测试全部"
    : "测试全部模型入口";
  testAllModels.title = "所有上游服务共享最多 3 个并发测试";
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
      ),
      "准备测试…",
    );
  });
  const testSummary = document.createElement("span");
  testSummary.className = "provider-test-summary";
  if (testSession) {
    const passed = allVirtualModels.filter(
      (virtualModel) => connectionTestResults.get(virtualModel.id)?.status === "success",
    ).length;
    testSummary.classList.add(failedVirtualModels.length > 0 ? "error" : "success");
    testSummary.textContent = `${passed}/${allVirtualModels.length} 通过`;
    testSummary.title = `最近测试：${formatActivityTime(testSession.completedAt).label} · ${passed} 通过 · ${failedVirtualModels.length} 失败`;
    providerTestActions.append(testSummary);
  }
  providerTestActions.append(testAllModels);
  providerActions.append(providerEditActions, providerTestActions);

  const models = document.createElement("div");
  models.className = "provider-models";
  if (modelLinks.length === 0) {
    const empty = document.createElement("p");
    empty.className = "provider-model-empty";
    empty.textContent = "尚未配置模型";
    models.append(empty);
  } else {
    const modelsHeader = document.createElement("div");
    modelsHeader.className = "provider-models-header";
    for (const label of ["上游模型", "模型能力", "模型入口"]) {
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
