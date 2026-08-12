import type { ProviderCatalogModel } from "../../types/catalog";
import type { ModelModality, UpstreamModel } from "../../types/config";

export type MultimodalInputModality = Exclude<ModelModality, "text">;

export const MULTIMODAL_INPUT_MODALITIES: readonly MultimodalInputModality[] = [
  "image",
  "document",
  "audio",
  "video",
];

export const DEFAULT_MULTIMODAL_MIME_TYPES: Readonly<
Record<MultimodalInputModality, readonly string[]>
> = {
  image: [
    "image/heic",
    "image/heif",
    "image/jpeg",
    "image/png",
    "image/webp",
  ],
  document: [
    "application/json",
    "application/pdf",
    "application/rtf",
    "application/x-ipynb+json",
    "application/x-javascript",
    "application/x-python-code",
    "application/x-typescript",
    "text/css",
    "text/csv",
    "text/html",
    "text/javascript",
    "text/markdown",
    "text/plain",
    "text/rtf",
    "text/x-python",
    "text/x-python-script",
    "text/x-typescript",
    "text/xml",
  ],
  audio: [
    "audio/webm;codecs=opus",
    "video/audio/s16le",
    "video/audio/wav",
  ],
  video: [
    "video/jpeg2000",
    "video/mp4",
    "video/text/timestamp",
    "video/videoframe/jpeg2000",
    "video/webm",
  ],
};

const DEFAULT_MIME_TYPE_MODALITIES = new Map<string, MultimodalInputModality>(
  MULTIMODAL_INPUT_MODALITIES.flatMap((modality) => (
    DEFAULT_MULTIMODAL_MIME_TYPES[modality].map((mimeType) => [mimeType, modality] as const)
  )),
);

export function normalizeInputMimeTypes(mimeTypes: Iterable<string> | undefined): string[] {
  if (!mimeTypes) return [];
  const normalized: string[] = [];
  const seen = new Set<string>();
  for (const mimeType of mimeTypes) {
    const value = mimeType.trim().toLowerCase();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    normalized.push(value);
  }
  return normalized;
}

function inputModalityForMimeType(mimeType: string): MultimodalInputModality {
  const normalized = mimeType.trim().toLowerCase();
  const declared = DEFAULT_MIME_TYPE_MODALITIES.get(normalized);
  if (declared) return declared;
  if (normalized.startsWith("image/")) return "image";
  if (normalized.startsWith("audio/") || normalized.startsWith("video/audio/")) return "audio";
  if (normalized.startsWith("video/")) return "video";
  return "document";
}

export function catalogSupportsInput(
  model: ProviderCatalogModel,
  modality: ModelModality,
): boolean {
  return model.inputModalities?.includes(modality) ?? modality === "text";
}

export function upstreamSupportsInput(
  upstream: UpstreamModel,
  modality: ModelModality,
): boolean {
  return upstream.capabilities.input_modalities.includes(modality);
}

export function catalogInputMimeTypes(model: ProviderCatalogModel): string[] {
  return normalizeInputMimeTypes(model.inputMimeTypes);
}

export function upstreamInputMimeTypes(upstream: UpstreamModel): string[] {
  return normalizeInputMimeTypes(upstream.capabilities.input_mime_types);
}

export function normalizeSelectedInputMimeTypes(
  mimeTypes: Iterable<string>,
  selectedModalities: ReadonlySet<MultimodalInputModality>,
): string[] {
  const selectedMimeTypes = normalizeInputMimeTypes(mimeTypes)
    .filter((mimeType) => selectedModalities.has(inputModalityForMimeType(mimeType)));
  for (const modality of MULTIMODAL_INPUT_MODALITIES) {
    if (selectedModalities.has(modality)) {
      selectedMimeTypes.push(...DEFAULT_MULTIMODAL_MIME_TYPES[modality]);
    }
  }
  return normalizeInputMimeTypes(selectedMimeTypes);
}
