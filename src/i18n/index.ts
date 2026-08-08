import { zhCN, type TranslationDict } from "./locales/zh-CN";
import { enUS } from "./locales/en-US";

export type SupportedLocale = "zh-CN" | "en-US";

type TranslationSection = keyof TranslationDict & string;
export type TranslationKey = {
  [Section in TranslationSection]: `${Section}.${keyof TranslationDict[Section] & string}`;
}[TranslationSection];

const LOCALES = {
  "zh-CN": zhCN,
  "en-US": enUS,
} satisfies Record<SupportedLocale, TranslationDict>;

type Listener = (lang: SupportedLocale) => void;
const listeners = new Set<Listener>();

function getInitialLanguage(): SupportedLocale {
  const saved = localStorage.getItem("agy_language") as SupportedLocale | null;
  if (saved && saved in LOCALES) return saved;

  const navLang = navigator.language;
  if (/^zh/i.test(navLang)) return "zh-CN";
  return "en-US";
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

function lookup(dict: TranslationDict, keys: readonly string[]): string | undefined {
  let value: unknown = dict;
  for (const key of keys) {
    if (typeof value !== "object" || value === null) return undefined;
    value = (value as Record<string, unknown>)[key];
  }
  return typeof value === "string" ? value : undefined;
}

export function isTranslationKey(path: string): path is TranslationKey {
  return lookup(zhCN, path.split(".")) !== undefined;
}

export function t(path: TranslationKey, params?: Record<string, string | number>): string {
  const keys = path.split(".");
  let text = lookup(LOCALES[currentLanguage], keys) ?? "—";

  if (params) {
    for (const [pKey, pVal] of Object.entries(params)) {
      text = text.split(`{${pKey}}`).join(String(pVal));
    }
  }

  return text;
}

function translateDomKey(key: string): string {
  if (isTranslationKey(key)) return t(key);
  console.error(`Unknown i18n key: ${key}`);
  return "—";
}

function canUpdateTextContent(element: HTMLElement): boolean {
  return element.dataset.busy !== "true" && element.dataset.armed !== "true";
}

export function updateDOMTranslations(): void {
  const elements = document.querySelectorAll<HTMLElement>("[data-i18n]");
  for (const el of elements) {
    const key = el.dataset.i18n;
    if (key && canUpdateTextContent(el)) {
      el.textContent = translateDomKey(key);
    }
  }

  const inputs = document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-i18n-placeholder]");
  for (const el of inputs) {
    const key = el.dataset.i18nPlaceholder;
    if (key) {
      el.placeholder = translateDomKey(key);
    }
  }

  const titledElements = document.querySelectorAll<HTMLElement>("[data-i18n-title]");
  for (const el of titledElements) {
    const key = el.dataset.i18nTitle;
    if (key) el.title = translateDomKey(key);
  }

  const ariaLabelledElements = document.querySelectorAll<HTMLElement>("[data-i18n-aria-label]");
  for (const el of ariaLabelledElements) {
    const key = el.dataset.i18nAriaLabel;
    if (key) el.setAttribute("aria-label", translateDomKey(key));
  }

  const contentElements = document.querySelectorAll<HTMLElement>("[data-i18n-content]");
  for (const el of contentElements) {
    const key = el.dataset.i18nContent;
    if (key) el.setAttribute("content", translateDomKey(key));
  }
}
