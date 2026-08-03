import type { Provider } from "../../types/config";
import type { ModelConnectionTestResult } from "../../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningLevel } from "../../types/reasoning";
import { testProviderModelConnection as testProviderModelConnectionCommand } from "../../controllers/providerController";
import { reasoningLevelLabel, sortReasoningLevels } from "../../utils/reasoningUtils";
import { t } from "../../i18n";

export async function testProviderModelConnection(
  provider: Provider,
  upstreamModelId: string,
  reasoningLevel: ReasoningLevel | null,
  customReasoningValue: string | null,
): Promise<ModelConnectionTestResult> {
  return testProviderModelConnectionCommand(
    provider,
    upstreamModelId,
    reasoningLevel,
    customReasoningValue,
  );
}

export interface CatalogModelTestContext {
  button: HTMLButtonElement;
  result: HTMLSpanElement;
  modelId: string;
  providerFromForm: () => Provider;
  isReasoningEnabled: () => boolean;
  selectedReasoningLevels: () => ReadonlySet<ConfigurableReasoningLevel>;
  runBusy: (
    button: HTMLButtonElement,
    action: () => Promise<void>,
    busyLabel?: string,
  ) => Promise<void>;
}

export function runCatalogModelTests(context: CatalogModelTestContext): void {
  void context.runBusy(context.button, async () => {
    const provider = context.providerFromForm();
    const testCases: Array<{
      label: string;
      reasoningLevel: ReasoningLevel | null;
    }> = [];
    if (context.isReasoningEnabled()) {
      for (const level of sortReasoningLevels(context.selectedReasoningLevels())) {
        testCases.push({ label: reasoningLevelLabel(level), reasoningLevel: level });
      }
    }
    if (testCases.length === 0) {
      testCases.push({ label: t("models.normalRequest"), reasoningLevel: null });
    }

    const results: string[] = [];
    let allSucceeded = true;
    let failedCount = 0;
    for (const [index, testCase] of testCases.entries()) {
      context.result.className = "catalog-model-test-result pending";
      context.result.textContent = t("models.testProgress", {
        current: index + 1,
        total: testCases.length,
        label: testCase.label,
      });
      const response = await testProviderModelConnection(
        provider,
        context.modelId,
        testCase.reasoningLevel,
        null,
      );
      allSucceeded = allSucceeded && response.success;
      if (!response.success) failedCount += 1;
      results.push(response.success
        ? t("models.testCasePassed", { label: testCase.label, time: response.durationMs })
        : t("models.testCaseFailed", { label: testCase.label, msg: response.message }));
    }
    context.result.className = `catalog-model-test-result ${allSucceeded ? "success" : "error"}`;
    context.result.textContent = allSucceeded
      ? t("models.allTestsPassed", { count: testCases.length })
      : t("models.testsCompleted", { failed: failedCount });
    context.result.title = results.join("\n");
  }, t("models.testing"));
}
