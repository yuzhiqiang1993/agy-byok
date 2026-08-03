import { zhCN, type TranslationDict } from "./locales/zh-CN";
import { zhTW } from "./locales/zh-TW";
import { enUS } from "./locales/en-US";
import { jaJP } from "./locales/ja-JP";
import { koKR } from "./locales/ko-KR";
import { esES } from "./locales/es-ES";
import { frFR } from "./locales/fr-FR";
import { deDE } from "./locales/de-DE";
import { ruRU } from "./locales/ru-RU";
import { ptBR } from "./locales/pt-BR";

export type SupportedLocale =
  | "zh-CN"
  | "zh-TW"
  | "en-US"
  | "ja-JP"
  | "ko-KR"
  | "es-ES"
  | "fr-FR"
  | "de-DE"
  | "ru-RU"
  | "pt-BR";

export const LOCALES: Record<SupportedLocale, TranslationDict> = {
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  "en-US": enUS,
  "ja-JP": jaJP,
  "ko-KR": koKR,
  "es-ES": esES,
  "fr-FR": frFR,
  "de-DE": deDE,
  "ru-RU": ruRU,
  "pt-BR": ptBR,
};

type Listener = (lang: SupportedLocale) => void;
const listeners = new Set<Listener>();

function getInitialLanguage(): SupportedLocale {
  const saved = localStorage.getItem("agy_language") as SupportedLocale | null;
  if (saved && saved in LOCALES) return saved;

  const navLang = navigator.language;
  if (/^zh-TW|^zh-HK|^zh-MO/i.test(navLang)) return "zh-TW";
  if (/^zh/i.test(navLang)) return "zh-CN";
  if (/^ja/i.test(navLang)) return "ja-JP";
  if (/^ko/i.test(navLang)) return "ko-KR";
  if (/^es/i.test(navLang)) return "es-ES";
  if (/^fr/i.test(navLang)) return "fr-FR";
  if (/^de/i.test(navLang)) return "de-DE";
  if (/^ru/i.test(navLang)) return "ru-RU";
  if (/^pt/i.test(navLang)) return "pt-BR";
  return "zh-CN";
}

let currentLanguage: SupportedLocale = getInitialLanguage();
document.documentElement.lang = currentLanguage;

export function getLanguage(): SupportedLocale {
  return currentLanguage;
}

export function setLanguage(lang: SupportedLocale): void {
  if (!(lang in LOCALES)) return;
  currentLanguage = lang;
  document.documentElement.lang = lang;
  localStorage.setItem("agy_language", lang);
  updateDOMTranslations();
  listeners.forEach((l) => l(lang));
}

export function subscribeLanguage(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function lookup(dict: TranslationDict, keys: string[]): string | undefined {
  let value: unknown = dict;
  for (const key of keys) {
    if (typeof value !== "object" || value === null) return undefined;
    value = (value as Record<string, unknown>)[key];
  }
  return typeof value === "string" ? value : undefined;
}

export function t(path: string, params?: Record<string, string | number>): string {
  const keys = path.split(".");
  const dictionaries = [LOCALES[currentLanguage], enUS, zhCN];
  let text = dictionaries.reduce<string | undefined>(
    (translation, dict) => translation ?? lookup(dict, keys),
    undefined,
  ) ?? "—";

  if (params) {
    for (const [pKey, pVal] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{${pKey}\\}`, "g"), String(pVal));
    }
  }

  return text;
}

export function updateDOMTranslations(): void {
  const elements = document.querySelectorAll<HTMLElement>("[data-i18n]");
  for (const el of elements) {
    const key = el.dataset.i18n;
    if (key) {
      el.textContent = t(key);
    }
  }

  const inputs = document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-i18n-placeholder]");
  for (const el of inputs) {
    const key = el.dataset.i18nPlaceholder;
    if (key) {
      el.placeholder = t(key);
    }
  }

  const titledElements = document.querySelectorAll<HTMLElement>("[data-i18n-title]");
  for (const el of titledElements) {
    const key = el.dataset.i18nTitle;
    if (key) el.title = t(key);
  }

  const ariaLabelledElements = document.querySelectorAll<HTMLElement>("[data-i18n-aria-label]");
  for (const el of ariaLabelledElements) {
    const key = el.dataset.i18nAriaLabel;
    if (key) el.setAttribute("aria-label", t(key));
  }
}
