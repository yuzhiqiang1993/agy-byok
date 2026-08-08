import type { VirtualModel } from "../../types/config";
import type {
  ConnectionTestViewState,
  ModelConnectionTestOutcome,
} from "../../types/proxy";
import { testVirtualModelConnection as testVirtualModelConnectionCommand } from "../../controllers/providerController";
import { t } from "../../i18n";
import { store } from "../../store/appStore";
import { connectionTestErrorMessage } from "../../utils/connectionTestUtils";
import { errorMessage } from "../../utils/errorUtils";
import {
  connectionTestResults,
  connectionTestsInFlight,
  providerTestSessions,
} from "./providerState";

const MAX_CONCURRENT_CONNECTION_TESTS = 3;
const connectionTestWaiters: Array<() => void> = [];
let activeConnectionTests = 0;

async function withConnectionTestSlot<T>(action: () => Promise<T>): Promise<T> {
  if (activeConnectionTests < MAX_CONCURRENT_CONNECTION_TESTS) {
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

export function renderConnectionTestState(
  target: HTMLElement,
  state: ConnectionTestViewState,
): void {
  const message = state.status === "testing"
    ? t("models.testing")
    : state.status === "success"
      ? t("models.testSuccess", { time: state.durationMs })
      : t("models.testFailed", {
          msg: typeof state.error === "string"
            ? state.error
            : connectionTestErrorMessage(state.error),
        });
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
  if (!store.config.virtual_models.some((model) => model.id === virtualModelId)) {
    connectionTestResults.delete(virtualModelId);
    return false;
  }
  if (outcome.kind === "result") {
    const state: ConnectionTestViewState = outcome.result.success
      ? { status: "success", durationMs: outcome.result.durationMs }
      : { status: "error", error: outcome.result };
    connectionTestResults.set(virtualModelId, state);
    renderConnectionTestState(target, state);
    return outcome.result.success;
  }

  const state: ConnectionTestViewState = {
    status: "error",
    error: outcome.message,
  };
  connectionTestResults.set(virtualModelId, state);
  renderConnectionTestState(target, state);
  return false;
}

interface ProviderModelTestOptions {
  providerId: string;
  card: HTMLElement;
  virtualModels: VirtualModel[];
  sessionVirtualModelIds: string[];
  progressButton: HTMLButtonElement;
  notify: (message: string, kind?: "success" | "error") => void;
  onChanged: () => void;
}

export async function testProviderModels(options: ProviderModelTestOptions): Promise<void> {
  const rows = [...options.card.querySelectorAll<HTMLElement>(".provider-model-variant")];
  const resultTargets = new Map(rows.map((row) => [
    row.dataset.virtualModelId,
    row.querySelector<HTMLElement>(".connection-result"),
  ]));
  let nextIndex = 0;
  let completed = 0;
  let succeeded = 0;
  const worker = async () => {
    while (nextIndex < options.virtualModels.length) {
      const virtualModel = options.virtualModels[nextIndex];
      nextIndex += 1;
      const target = resultTargets.get(virtualModel.id);
      if (target && await testVirtualModelConnection(virtualModel.id, target)) {
        succeeded += 1;
      }
      completed += 1;
      options.progressButton.textContent = t("models.testProgressSimple", {
        current: completed,
        total: options.virtualModels.length,
      });
    }
  };

  const concurrency = Math.min(MAX_CONCURRENT_CONNECTION_TESTS, options.virtualModels.length);
  await Promise.all(Array.from({ length: concurrency }, worker));

  if (!store.config.providers.some((provider) => provider.id === options.providerId)) return;
  const failed = options.virtualModels.length - succeeded;
  providerTestSessions.set(options.providerId, {
    targetVirtualModelIds: options.sessionVirtualModelIds,
    completedAt: Date.now(),
  });
  options.notify(
    t("models.testsSummary", { succeeded, failed }),
    failed > 0 ? "error" : "success",
  );
  window.setTimeout(options.onChanged, 0);
}
