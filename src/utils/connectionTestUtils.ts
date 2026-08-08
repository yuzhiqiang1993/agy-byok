import { t, type TranslationKey } from "../i18n";
import type {
  ModelConnectionErrorCategory,
  ModelConnectionTestResult,
} from "../types/proxy";

const ERROR_TRANSLATIONS: Record<ModelConnectionErrorCategory, TranslationKey> = {
  authentication: "models.connectionAuthentication",
  invalid_request: "models.connectionInvalidRequest",
  context_length_exceeded: "models.connectionContextLengthExceeded",
  rate_limit: "models.connectionRateLimit",
  model_not_found: "models.connectionModelNotFound",
  upstream_server_error: "models.connectionUpstreamServerError",
  timeout: "models.connectionTimeout",
  connection_failed: "models.connectionFailed",
  stream_interrupted: "models.connectionStreamInterrupted",
  unsupported_feature: "models.connectionUnsupportedFeature",
  internal: "models.connectionInvalidResponse",
  invalid_configuration: "models.connectionInvalidConfiguration",
};

export function connectionTestErrorMessage(result: ModelConnectionTestResult): string {
  const key = result.errorCategory
    ? ERROR_TRANSLATIONS[result.errorCategory]
    : "models.connectionUnknownError";
  const message = t(key);
  return result.statusCode
    ? t("models.connectionHttpStatus", { message, status: result.statusCode })
    : message;
}
