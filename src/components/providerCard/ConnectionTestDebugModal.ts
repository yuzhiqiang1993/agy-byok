import { createModal } from "../common/Modal";
import type { ModelConnectionTestResult } from "../../types/proxy";
import { t } from "../../i18n";
import { connectionTestErrorMessage } from "../../utils/connectionTestUtils";

export function showConnectionTestDebugModal(testCases: Array<{
  label: string;
  response: ModelConnectionTestResult;
}>): void {
  const container = document.createElement("div");
  container.className = "connection-test-debug-container";

  for (const { label, response } of testCases) {
    const caseSection = document.createElement("div");
    caseSection.className = "connection-test-debug-case";

    const header = document.createElement("h4");
    header.textContent = response.success
      ? t("models.testCasePassed", { label, time: response.durationMs })
      : t("models.testCaseFailed", {
          label,
          msg: response.errorMessage || connectionTestErrorMessage(response),
        });
    header.className = response.success ? "success-text" : "error-text";
    caseSection.appendChild(header);

    if (!response.success && response.errorCategory) {
      const errorDiv = document.createElement("div");
      errorDiv.className = "debug-error-message";
      errorDiv.textContent = connectionTestErrorMessage(response);
      caseSection.appendChild(errorDiv);
    }

    if (response.statusCode) {
      const statusDiv = document.createElement("div");
      statusDiv.className = "debug-status-code";
      statusDiv.textContent = `${t("models.httpStatus")}: ${response.statusCode}`;
      caseSection.appendChild(statusDiv);
    }

    if (response.requestBody) {
      const reqHeader = document.createElement("strong");
      reqHeader.textContent = t("models.requestPayload");
      caseSection.appendChild(reqHeader);
      
      const reqPre = document.createElement("pre");
      reqPre.textContent = response.requestBody;
      caseSection.appendChild(reqPre);
    }

    if (response.responseBody) {
      const resHeader = document.createElement("strong");
      resHeader.textContent = t("models.responseBody");
      caseSection.appendChild(resHeader);
      
      const resPre = document.createElement("pre");
      resPre.textContent = response.responseBody;
      caseSection.appendChild(resPre);
    }
    
    container.appendChild(caseSection);
  }

  createModal({
    title: t("models.testDebugDetails"),
    body: container,
    cancelLabel: t("common.close") || "Close",
    dialogClassName: "connection-test-debug-modal",
  });
}
