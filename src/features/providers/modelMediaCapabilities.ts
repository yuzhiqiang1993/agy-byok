import type { ProviderCatalogModel } from "../../types/catalog";
import type {
  ModelModality,
  ProviderProtocol,
  UpstreamModel,
} from "../../types/config";

export const DEFAULT_IMAGE_MIME_TYPES = ["image/png", "image/jpeg", "image/webp"] as const;
export const DEFAULT_AUDIO_MIME_TYPES = ["audio/wav"] as const;
export const DEFAULT_VIDEO_MIME_TYPES = ["video/mp4", "video/webm"] as const;
export const DEFAULT_DOCUMENT_MIME_TYPES = ["application/pdf"] as const;

type BinaryInputModality = Exclude<ModelModality, "text">;

const DEFAULT_MIME_TYPES: Record<BinaryInputModality, readonly string[]> = {
  image: DEFAULT_IMAGE_MIME_TYPES,
  audio: DEFAULT_AUDIO_MIME_TYPES,
  video: DEFAULT_VIDEO_MIME_TYPES,
  document: DEFAULT_DOCUMENT_MIME_TYPES,
};

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

export function hasMimeTypeCategory(
  mimeTypes: Iterable<string>,
  category: BinaryInputModality,
): boolean {
  return [...mimeTypes].some((mimeType) => matchesInputModality(mimeType, category));
}

function matchesInputModality(mimeType: string, modality: BinaryInputModality): boolean {
  // 文档不是独立的 MIME 一级类型，当前仅支持 PDF。
  if (modality === "document") {
    return (DEFAULT_DOCUMENT_MIME_TYPES as readonly string[]).includes(mimeType);
  }
  return mimeType.startsWith(`${modality}/`);
}

function withoutMimeTypeCategory(
  mimeTypes: Iterable<string>,
  category: BinaryInputModality,
): string[] {
  return normalizeInputMimeTypes(mimeTypes)
    .filter((mimeType) => !matchesInputModality(mimeType, category));
}

function withDefaultMimeTypes(
  mimeTypes: Iterable<string>,
  modality: BinaryInputModality,
): string[] {
  return normalizeInputMimeTypes([...mimeTypes, ...DEFAULT_MIME_TYPES[modality]]);
}

export function supportsInputModality(
  protocol: ProviderProtocol,
  modality: BinaryInputModality,
): boolean {
  if (modality === "image" || modality === "document") return true;
  if (modality === "audio") {
    return protocol === "openai_chat_completions" || protocol === "gemini_generate_content";
  }
  return protocol === "gemini_generate_content";
}

function supportsInputMimeType(protocol: ProviderProtocol, mimeType: string): boolean {
  if (protocol === "gemini_generate_content") return true;
  if ((DEFAULT_IMAGE_MIME_TYPES as readonly string[]).includes(mimeType)) return true;
  if ((DEFAULT_DOCUMENT_MIME_TYPES as readonly string[]).includes(mimeType)) return true;
  return protocol === "openai_chat_completions"
    && ["audio/wav", "audio/x-wav", "audio/mpeg", "audio/mp3"].includes(mimeType);
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
  options: {
    selectedModalities: ReadonlySet<BinaryInputModality>;
    protocol: ProviderProtocol;
  },
): string[] {
  let normalized = normalizeInputMimeTypes(mimeTypes);
  // 目录能力还需经过当前协议适配器的实际编码范围过滤。
  normalized = normalized.filter((mimeType) => supportsInputMimeType(options.protocol, mimeType));
  for (const modality of ["image", "audio", "video", "document"] as const) {
    if (options.selectedModalities.has(modality)
      && supportsInputModality(options.protocol, modality)) {
      if (!hasMimeTypeCategory(normalized, modality)) {
        normalized = withDefaultMimeTypes(normalized, modality);
      }
    } else {
      normalized = withoutMimeTypeCategory(normalized, modality);
    }
  }
  return normalized;
}
