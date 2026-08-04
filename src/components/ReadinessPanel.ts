import { element, errorMessage } from "../utils/domUtils";
import { store } from "../store/appStore";
import { clientConfigurationReady, clientReady } from "../utils/displayUtils";
import { startProxy } from "../controllers/proxyController";
import { refreshHostStatuses } from "../controllers/hostController";
import { switchTab } from "./TabManager";
import { showNotice } from "./NoticeBar";
import { t } from "../i18n";

function setReadinessStep(
  selector: string,
  valueSelector: string,
  state: "pending" | "ready" | "attention",
  value: string,
): void {
  element<HTMLLIElement>(selector).dataset.state = state;
  element<HTMLElement>(valueSelector).textContent = value;
}

export function renderReadiness(): void {
  const modelCountValue = store.config?.virtual_models.length ?? 0;
  const proxyRunning = store.proxyStatus?.state === "running";

  const latestProxyStatus = store.proxyStatus;
  const latestIdeStatus = store.ideStatus;
  const latestAppStatus = store.appStatus;
  const latestCliStatus = store.cliStatus;

  const proxyStatusLoadFailed = store.proxyStatusLoadFailed;
  const ideStatusLoadFailed = store.ideStatusLoadFailed;
  const appStatusLoadFailed = store.appStatusLoadFailed;
  const cliStatusLoadFailed = store.cliStatusLoadFailed;

  const ideReady = latestIdeStatus
    ? latestIdeStatus.compatible
      && clientReady(latestIdeStatus.integrationState)
      && clientConfigurationReady(latestIdeStatus.configurationState, proxyRunning)
    : false;
  const appReady = latestAppStatus
    ? latestAppStatus.installed
      && latestAppStatus.integrationState === "managed"
      && clientConfigurationReady(latestAppStatus.configurationState, proxyRunning)
    : false;
  const cliReady = latestCliStatus
    ? latestCliStatus.installed
      && latestCliStatus.integrationState === "managed"
      && clientConfigurationReady(latestCliStatus.configurationState, proxyRunning)
    : false;

  const enabledClients = [
    ideReady ? "IDE" : null,
    appReady ? "App" : null,
    cliReady ? "CLI" : null,
  ].filter((item): item is string => item !== null);
  const entryStatusesLoadFailed = ideStatusLoadFailed || appStatusLoadFailed || cliStatusLoadFailed;
  const entryStatusesLoading = latestIdeStatus === null || latestAppStatus === null || latestCliStatus === null;

  setReadinessStep(
    "#readiness-models",
    "#readiness-models-value",
    modelCountValue > 0 ? "ready" : "attention",
    modelCountValue > 0 ? t("overview.step1Configured", { count: modelCountValue }) : t("overview.step1Action") + " →",
  );
  setReadinessStep(
    "#readiness-proxy",
    "#readiness-proxy-value",
    proxyStatusLoadFailed
      ? "attention"
      : latestProxyStatus === null
        ? "pending"
        : modelCountValue === 0
          ? "pending"
          : proxyRunning
            ? "ready"
            : "attention",
    proxyStatusLoadFailed
      ? t("overview.loadFailed")
      : latestProxyStatus === null
        ? t("overview.checking")
        : modelCountValue === 0
          ? t("overview.step1Unconfigured")
          : proxyRunning
            ? t("overview.proxyRunning")
            : t("overview.step2Action") + " →",
  );
  setReadinessStep(
    "#readiness-entry",
    "#readiness-entry-value",
    entryStatusesLoadFailed
      ? "attention"
      : entryStatusesLoading
        ? "pending"
        : modelCountValue === 0 || !proxyRunning
          ? "pending"
          : enabledClients.length > 0
            ? "ready"
            : "attention",
    entryStatusesLoadFailed
      ? t("overview.loadFailed")
      : entryStatusesLoading
        ? t("overview.checking")
        : modelCountValue === 0
          ? t("overview.step1Unconfigured")
          : !proxyRunning
            ? t("overview.step2Stopped")
            : enabledClients.length > 0
              ? t("overview.step3Ready", { count: enabledClients.length })
              : t("overview.step3Action") + " →",
  );
  setReadinessStep(
    "#readiness-restore",
    "#readiness-restore-value",
    "ready",
    t("overview.step4Value"),
  );

  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  if (modelCountValue === 0) {
    title.textContent = t("overview.step1HeaderTitle");
    detail.textContent = t("overview.step1HeaderDesc");
  } else if (proxyStatusLoadFailed || entryStatusesLoadFailed) {
    title.textContent = t("overview.loadFailed");
    detail.textContent = t("overview.loadFailed");
  } else if (latestProxyStatus === null || entryStatusesLoading) {
    title.textContent = t("overview.checkingStatusTitle");
    detail.textContent = t("overview.checkingStatusDetail");
  } else if (!proxyRunning) {
    title.textContent = t("overview.step2HeaderTitle");
    detail.textContent = t("overview.step2HeaderDesc");
  } else if (enabledClients.length === 0) {
    title.textContent = t("overview.step3HeaderTitle");
    detail.textContent = t("overview.step3HeaderDesc");
  } else {
    title.textContent = t("overview.step4HeaderTitle");
    detail.textContent = t("overview.step4HeaderDesc");
  }
}

export function setupReadinessPanel(): void {
  const modelsStep = document.querySelector<HTMLElement>("#readiness-models");
  const proxyStep = document.querySelector<HTMLElement>("#readiness-proxy");
  const entryStep = document.querySelector<HTMLElement>("#readiness-entry");

  if (modelsStep) {
    modelsStep.title = t("overview.readinessModelsTooltip");
    modelsStep.addEventListener("click", () => {
      void switchTab("tab-models");
    });
  }

  if (proxyStep) {
    proxyStep.title = t("overview.readinessProxyTooltip");
    proxyStep.addEventListener("click", () => {
      const modelCount = store.config?.virtual_models.length ?? 0;
      if (modelCount === 0) {
        showNotice(t("overview.proxyModelsRequired", { count: 1 }), "error");
        void switchTab("tab-models");
        return;
      }
      const proxyRunning = store.proxyStatus?.state === "running";
      if (!proxyRunning) {
        void startProxy()
          .then(() => refreshHostStatuses())
          .then(() => showNotice(t("overview.proxyStarted")))
          .catch((error: unknown) => showNotice(errorMessage(error), "error"));
      } else {
        showNotice(t("overview.proxyAlreadyRunning"));
      }
    });
  }

  if (entryStep) {
    entryStep.title = t("overview.readinessEntryTooltip");
    entryStep.addEventListener("click", () => {
      const modelCount = store.config?.virtual_models.length ?? 0;
      if (modelCount === 0) {
        showNotice(t("overview.hostModelsRequired", { count: 1 }), "error");
        void switchTab("tab-models");
        return;
      }
      showNotice(t("overview.hostEntryPrompt"));
      const section = document.querySelector("#host-cards-section");
      if (section) {
        section.scrollIntoView({ behavior: "smooth" });
        const cards = document.querySelectorAll(".status-card");
        cards.forEach((card) => {
          card.classList.remove("highlight-pulse");
          void (card as HTMLElement).offsetWidth;
          card.classList.add("highlight-pulse");
        });
        setTimeout(() => {
          cards.forEach((card) => card.classList.remove("highlight-pulse"));
        }, 1250);
      }
    });
  }
}

