import type { ProviderCatalogModel } from "../../types/catalog";
import type { ProviderProtocol, UpstreamModel } from "../../types/config";

export const DEFAULT_IMAGE_MIME_TYPES = ["image/png", "image/jpeg", "image/webp"] as const;
export const DEFAULT_VIDEO_MIME_TYPES = ["video/mp4", "video/webm"] as const;

function catalogCapability(
  model: ProviderCatalogModel,
  name: "vision",
): boolean | undefined {
  const capabilities = model.capabilities;
  if (!capabilities || Array.isArray(capabilities)) return undefined;
  const value = capabilities[name];
  return typeof value === "boolean" ? value : undefined;
}

export function normalizeSupportedMimeTypes(mimeTypes: Iterable<string> | undefined): string[] {
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
  category: "image" | "video",
): boolean {
  return [...mimeTypes].some((mimeType) => mimeType.startsWith(`${category}/`));
}

function withoutMimeTypeCategory(
  mimeTypes: Iterable<string>,
  category: "image" | "video",
): string[] {
  return normalizeSupportedMimeTypes(mimeTypes)
    .filter((mimeType) => !mimeType.startsWith(`${category}/`));
}

function withDefaultMimeTypes(
  mimeTypes: Iterable<string>,
  defaults: readonly string[],
): string[] {
  return normalizeSupportedMimeTypes([...mimeTypes, ...defaults]);
}

export function supportsVideoInput(protocol: ProviderProtocol): boolean {
  return protocol === "gemini_generate_content";
}

export function catalogSupportsImages(model: ProviderCatalogModel): boolean {
  if (typeof model.supportsImages === "boolean") return model.supportsImages;
  if (hasMimeTypeCategory(normalizeSupportedMimeTypes(model.supportedMimeTypes), "image")) return true;
  return catalogCapability(model, "vision") ?? false;
}

export function catalogSupportsVideo(model: ProviderCatalogModel): boolean {
  if (typeof model.supportsVideo === "boolean") return model.supportsVideo;
  return hasMimeTypeCategory(normalizeSupportedMimeTypes(model.supportedMimeTypes), "video");
}

export function catalogSupportedMimeTypes(model: ProviderCatalogModel): string[] {
  let mimeTypes = normalizeSupportedMimeTypes(model.supportedMimeTypes);
  if (catalogSupportsImages(model) && !hasMimeTypeCategory(mimeTypes, "image")) {
    mimeTypes = withDefaultMimeTypes(mimeTypes, DEFAULT_IMAGE_MIME_TYPES);
  }
  if (catalogSupportsVideo(model) && !hasMimeTypeCategory(mimeTypes, "video")) {
    mimeTypes = withDefaultMimeTypes(mimeTypes, DEFAULT_VIDEO_MIME_TYPES);
  }
  return mimeTypes;
}

export function upstreamSupportedMimeTypes(upstream: UpstreamModel): string[] {
  return normalizeSupportedMimeTypes(upstream.capabilities.supported_mime_types);
}

export function normalizeMediaMimeTypes(
  mimeTypes: Iterable<string>,
  options: {
    supportsImages: boolean;
    supportsVideo: boolean;
    videoAvailable: boolean;
  },
): string[] {
  let normalized = normalizeSupportedMimeTypes(mimeTypes);
  // 非 Gemini 适配器仅声明并转发已验证的图片格式。
  if (!options.videoAvailable) {
    const supportedImageMimeTypes = new Set<string>(DEFAULT_IMAGE_MIME_TYPES);
    normalized = normalized.filter((mimeType) => supportedImageMimeTypes.has(mimeType));
  }
  if (options.supportsImages) {
    if (!hasMimeTypeCategory(normalized, "image")) {
      normalized = withDefaultMimeTypes(normalized, DEFAULT_IMAGE_MIME_TYPES);
    }
  } else {
    normalized = withoutMimeTypeCategory(normalized, "image");
  }
  if (options.supportsVideo && options.videoAvailable) {
    normalized = withDefaultMimeTypes(normalized, DEFAULT_VIDEO_MIME_TYPES);
  } else {
    normalized = withoutMimeTypeCategory(normalized, "video");
  }
  return normalized;
}
