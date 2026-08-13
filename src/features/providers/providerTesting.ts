import type { Provider, UpstreamModel } from "../../types/config";
import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelConnectionTestResult } from "../../types/proxy";
import type { ConfigurableReasoningLevel, ReasoningLevel, ReasoningMapping } from "../../types/reasoning";
import { testProviderModelConnection as testProviderModelConnectionCommand } from "../../controllers/providerController";
import { reasoningLevelLabel, resolveReasoningMappingForModel, sortReasoningLevels } from "../../utils/reasoningUtils";
import { t } from "../../i18n";
import { showConnectionTestDebugModal } from "../../components/providerCard/ConnectionTestDebugModal";

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
  result: HTMLButtonElement;
  modelId: string;
  model: ProviderCatalogModel;
  existingUpstream?: UpstreamModel;
  providerFromForm: () => Provider;
  isReasoningEnabled: () => boolean;
  selectedReasoningLevels: () => ReadonlySet<ConfigurableReasoningLevel>;
  outputTokenLimit: () => number | null;
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
        const mapping = resolveReasoningMappingForModel(
          context.model,
          provider.protocol,
          level,
          context.existingUpstream,
          context.outputTokenLimit(),
        ).mapping;
        testCases.push({ label: reasoningLevelLabel(level), reasoningLevel: level, mapping });
      }
    }
    if (testCases.length === 0) {
      testCases.push({ label: t("models.normalRequest"), reasoningLevel: null, mapping: null });
    }

    const testCasesContext: Array<{ label: string; response: ModelConnectionTestResult }> = [];

    // 从测试一开始就绑定点击弹窗事件，随时可以点击查看已有结果
    context.result.removeAttribute("title");
    context.result.disabled = true;
    context.result.onclick = () => {
      if (testCasesContext.length > 0) {
        showConnectionTestDebugModal(testCasesContext);
      }
    };

    let allSucceeded = true;
    let failedCount = 0;

    try {
      for (const [index, testCase] of testCases.entries()) {
        context.result.className = "catalog-model-test-result pending";
        context.result.textContent = t("models.testProgress", {
          current: index + 1,
          total: testCases.length,
          label: testCase.label,
        });

        try {
          const response = await testProviderModelConnection(
            provider,
            context.modelId,
            testCase.reasoningLevel,
            null,
            testCase.mapping,
          );
          allSucceeded = allSucceeded && response.success;
          if (!response.success) failedCount += 1;
          testCasesContext.push({ label: testCase.label, response });
          context.result.disabled = false;
        } catch (err) {
          allSucceeded = false;
          failedCount += 1;
          const errMessage = err instanceof Error ? err.message : String(err);
          testCasesContext.push({
            label: testCase.label,
            response: {
              success: false,
              durationMs: 0,
              errorCategory: "internal",
              statusCode: null,
              requestBody: null,
              errorMessage: errMessage,
              responseBody: `Execution Exception: ${errMessage}`,
            },
          });
          context.result.disabled = false;
        }
      }
      context.result.className = `catalog-model-test-result ${allSucceeded ? "success" : "error"}`;
      context.result.textContent = allSucceeded
        ? t("models.allTestsPassed", { count: testCases.length })
        : t("models.testsCompleted", { failed: failedCount });
    } catch (outerErr) {
      context.result.className = "catalog-model-test-result error";
      context.result.textContent = t("models.testsCompleted", { failed: testCases.length });
      context.result.disabled = testCasesContext.length === 0;
    }
  }, t("models.testing"));
}
