export interface ProviderCatalogDebugResult {
  success: boolean;
  requestUrl: string;
  statusCode: number | null;
  contentType: string | null;
  errorCategory: string | null;
  errorMessage: string | null;
  responseBody: string;
}
