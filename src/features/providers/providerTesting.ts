import type { Provider, UpstreamModel } from "../../types/config";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelConnectionTestResult } from "../../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningLevel, ReasoningMapping } from "../../types/reasoning";
import { testProviderModelConnection as testProviderModelConnectionCommand } from "../../controllers/providerController";
import { catalogReasoningMappingsForModel, reasoningLevelLabel, sortReasoningLevels } from "../../utils/reasoningUtils";
import { connectionTestErrorMessage } from "../../utils/connectionTestUtils";
import { t } from "../../i18n";

export async function testProviderModelConnection(
  provider: Provider,
  upstreamModelId: string,
  reasoningLevel: ReasoningLevel | null,
  customReasoningValue: string | null,
  reasoningMapping: ReasoningMapping | null = null,
): Promise<ModelConnectionTestResult> {
  return testProviderModelConnectionCommand(
    provider,
    upstreamModelId,
    reasoningLevel,
    customReasoningValue,
    reasoningMapping,
  );
}

interface CatalogModelTestContext {
  button: HTMLButtonElement;
  result: HTMLSpanElement;
  modelId: string;
  model: ProviderCatalogModel;
  existingUpstream?: UpstreamModel;
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
      mapping: ReasoningMapping | null;
    }> = [];
    if (context.isReasoningEnabled()) {
      for (const level of sortReasoningLevels(context.selectedReasoningLevels())) {
        const mapping = context.model.reasoning?.mappings?.[level]
          ?? context.existingUpstream?.capabilities.reasoning.levels[level]
          ?? catalogReasoningMappingsForModel(context.model, provider.protocol)[level]
          ?? null;
        testCases.push({ label: reasoningLevelLabel(level), reasoningLevel: level, mapping });
      }
    }
    if (testCases.length === 0) {
      testCases.push({ label: t("models.normalRequest"), reasoningLevel: null, mapping: null });
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
        testCase.mapping,
      );
      allSucceeded = allSucceeded && response.success;
      if (!response.success) failedCount += 1;
      results.push(response.success
        ? t("models.testCasePassed", { label: testCase.label, time: response.durationMs })
        : t("models.testCaseFailed", {
            label: testCase.label,
            msg: connectionTestErrorMessage(response),
          }));
    }
    context.result.className = `catalog-model-test-result ${allSucceeded ? "success" : "error"}`;
    context.result.textContent = allSucceeded
      ? t("models.allTestsPassed", { count: testCases.length })
      : t("models.testsCompleted", { failed: failedCount });
    context.result.title = results.join("\n");
  }, t("models.testing"));
}
