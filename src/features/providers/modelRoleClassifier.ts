import type { ProviderCatalogModel } from "../../types/catalog";

export function isLikelyImageModel(model: ProviderCatalogModel): boolean {
  if (model.roles?.includes("image_generation") && !model.roles?.includes("agent")) {
    return true;
  }
  const text = `${model.id} ${model.displayName}`.toLowerCase();
  return (
    text.includes("flash-image")
    || text.includes("imagen")
    || text.includes("nano-banana-pro")
    || text.includes("image-generation")
    || text.includes("image_generation")
    || text.includes("image generation")
    || text.includes("text-to-image")
    || text.includes("text2image")
    || text.includes("image-to-image")
    || text.includes("image2image")
    || text.includes("text-to-video")
    || text.includes("text2video")
    || text.includes("dall-e")
    || text.includes("dalle")
    || text.includes("gpt-image")
    || text.includes("gpt image")
    || text.includes("gpt_image")
    || text.includes("flux")
    || text.includes("midjourney")
    || text.includes("sdxl")
    || text.includes("stable-diffusion")
    || text.includes("stable diffusion")
    || text.includes("stable_diffusion")
    || text.includes("stable-image")
    || text.includes("recraft")
    || text.includes("kolors")
    || text.includes("ideogram")
    || text.includes("kling")
    || text.includes("cogview")
    || text.includes("imagine")
    || text.includes("hunyuan-image")
    || text.includes("hunyuan-video")
    || text.includes("doubao-image")
    || text.includes("wanx")
    || /\bimage[-\s]?(?:1|2|3|4|5|v\d|\d+\.\d+)/i.test(text)
  );
}
