import { openExternalUrl as openExternalUrlCommand } from "../../controllers/hostController";
import { t, type TranslationKey } from "../../i18n";
import { showNotice } from "../NoticeBar";
import { openSettingsConfigDirectory } from "./DataSettings";

interface ExternalLink {
  selector: string;
  url: string;
  labelKey: TranslationKey;
}

const EXTERNAL_LINKS: ExternalLink[] = [
  {
    selector: "#about-card-github",
    url: "https://github.com/yuzhiqiang1993/agy-byok",
    labelKey: "settings.cardGithub",
  },
  {
    selector: "#about-card-author",
    url: "https://github.com/yuzhiqiang1993",
    labelKey: "settings.cardAuthor",
  },
  {
    selector: "#about-card-feedback",
    url: "https://github.com/yuzhiqiang1993/agy-byok/issues",
    labelKey: "settings.cardFeedback",
  },
];

async function openExternalUrl(url: string, label: string): Promise<void> {
  try {
    await openExternalUrlCommand(url);
    showNotice(t("settings.externalOpened", { label }));
  } catch {
    window.open(url, "_blank");
  }
}

export function setupAboutSettings(): void {
  document.querySelector("#about-card-dir")?.addEventListener(
    "click",
    () => void openSettingsConfigDirectory(),
  );
  for (const link of EXTERNAL_LINKS) {
    document.querySelector(link.selector)?.addEventListener("click", () => {
      void openExternalUrl(link.url, t(link.labelKey));
    });
  }
}
