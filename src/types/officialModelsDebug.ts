
export interface OfficialModelsDebugResult {
  success: boolean;
  source: string | null;
  requestUrl: string | null;
  statusCode: number | null;
  contentType: string | null;
  errorCategory: string | null;
  errorMessage: string | null;
  rawResponse: string | null;
  modifiedResponse: string | null;
}
