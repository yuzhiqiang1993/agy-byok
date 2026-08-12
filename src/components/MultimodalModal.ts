import { t } from "../i18n";
import type { ProviderCatalogModel } from "../types/catalog";
import {
  MULTIMODAL_INPUT_MODALITIES,
  type MultimodalInputModality,
} from "../features/providers/modelMediaCapabilities";
import { createModal, type ModalInstance } from "./common/Modal";

interface MultimodalModalContext {
  currentModalities: ReadonlySet<MultimodalInputModality>;
  onConfirm: (modalities: Set<MultimodalInputModality>) => void;
}

let currentModal: ModalInstance | null = null;

function modalityLabel(modality: MultimodalInputModality): string {
  return {
    image: t("models.visionInput"),
    document: t("models.documentInput"),
    audio: t("models.audioInput"),
    video: t("models.videoInput"),
  }[modality];
}

function modalityDescription(modality: MultimodalInputModality): string {
  return {
    image: t("models.multimodalImageHint"),
    document: t("models.multimodalDocumentHint"),
    audio: t("models.multimodalAudioHint"),
    video: t("models.multimodalVideoHint"),
  }[modality];
}

export function openMultimodalModal(
  model: ProviderCatalogModel,
  context: MultimodalModalContext,
): void {
  currentModal?.close();
  const draftModalities = new Set(context.currentModalities);
  const body = document.createElement("div");
  body.className = "multimodal-modal-options";

  const note = document.createElement("p");
  note.className = "multimodal-source-note";
  note.textContent = t("models.multimodalDeclarationHint");
  body.append(note);

  for (const modality of MULTIMODAL_INPUT_MODALITIES) {
    const label = document.createElement("label");
    label.className = "multimodal-modal-option";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = draftModalities.has(modality);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) draftModalities.add(modality);
      else draftModalities.delete(modality);
    });
    const copy = document.createElement("span");
    copy.className = "multimodal-modal-option-copy";
    const title = document.createElement("strong");
    title.textContent = modalityLabel(modality);
    const description = document.createElement("span");
    description.textContent = modalityDescription(modality);
    copy.append(title, description);
    label.append(checkbox, copy);
    body.append(label);
  }

  currentModal = createModal({
    title: `${t("models.multimodalConfig")} · ${model.displayName}`,
    subtitle: t("models.multimodalSubtitle"),
    body,
    dialogClassName: "multimodal-modal-dialog",
    okLabel: t("models.confirm"),
    cancelLabel: t("models.cancel"),
    onOk: () => {
      context.onConfirm(new Set(draftModalities));
      currentModal?.close();
    },
    onClosed: () => {
      currentModal = null;
    },
  });

  window.setTimeout(() => {
    body.querySelector<HTMLInputElement>("input")?.focus();
  }, 0);
}
