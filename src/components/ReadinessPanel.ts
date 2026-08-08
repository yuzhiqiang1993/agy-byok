import { element } from "../utils/domUtils";
import { errorMessage } from "../utils/errorUtils";
import { store } from "../store/appStore";
import { clientConfigurationReady, clientReady } from "../utils/displayUtils";
import { startProxy } from "../controllers/proxyController";
import { refreshHostStatuses } from "../controllers/hostController";
import { switchTab } from "./TabManager";
import { showNotice } from "./NoticeBar";
import { t } from "../i18n";

type ReadinessStepState = "pending" | "ready" | "attention";

interface ReadinessStepView {
  state: ReadinessStepState;
  value: string;
}

interface ReadinessSnapshot {
  modelCount: number;
  proxyRunning: boolean;
  proxyLoading: boolean;
  proxyLoadFailed: boolean;
  entryLoading: boolean;
  entryLoadFailed: boolean;
  readyClientCount: number;
}

function setReadinessStep(
  selector: string,
  valueSelector: string,
  view: ReadinessStepView,
): void {
  element<HTMLLIElement>(selector).dataset.state = view.state;
  element<HTMLElement>(valueSelector).textContent = view.value;
}

function readinessSnapshot(): ReadinessSnapshot {
  const proxyRunning = store.proxyStatus?.state === "running";
  const latestIdeStatus = store.ideStatus;
  const latestAppStatus = store.appStatus;
  const latestCliStatus = store.cliStatus;
  const ideReady = latestIdeStatus
    ? latestIdeStatus.compatible
      && clientReady(latestIdeStatus.integrationState)
      && clientConfigurationReady(latestIdeStatus.configurationState, proxyRunning)
    : false;
  const appReady = latestAppStatus
    ? latestAppStatus.installed
      && clientReady(latestAppStatus.integrationState)
      && clientConfigurationReady(latestAppStatus.configurationState, proxyRunning)
    : false;
  const cliReady = latestCliStatus
    ? latestCliStatus.installed
      && clientReady(latestCliStatus.integrationState)
      && clientConfigurationReady(latestCliStatus.configurationState, proxyRunning)
    : false;
  return {
    modelCount: store.config.virtual_models.length,
    proxyRunning,
    proxyLoading: store.proxyStatus === null,
    proxyLoadFailed: store.proxyStatusLoadFailed,
    entryLoading: latestIdeStatus === null || latestAppStatus === null || latestCliStatus === null,
    entryLoadFailed: store.ideStatusLoadFailed || store.appStatusLoadFailed || store.cliStatusLoadFailed,
    readyClientCount: [ideReady, appReady, cliReady].filter(Boolean).length,
  };
}

function modelsStep(snapshot: ReadinessSnapshot): ReadinessStepView {
  return snapshot.modelCount > 0
    ? {
        state: "ready",
        value: t("overview.step1Configured", { count: snapshot.modelCount }),
      }
    : { state: "attention", value: `${t("overview.step1Action")} →` };
}

function proxyStep(snapshot: ReadinessSnapshot): ReadinessStepView {
  if (snapshot.proxyLoadFailed) return { state: "attention", value: t("overview.loadFailed") };
  if (snapshot.proxyLoading) return { state: "pending", value: t("overview.checking") };
  if (snapshot.modelCount === 0) return { state: "pending", value: t("overview.step1Unconfigured") };
  return snapshot.proxyRunning
    ? { state: "ready", value: t("overview.proxyRunning") }
    : { state: "attention", value: `${t("overview.step2Action")} →` };
}

function entryStep(snapshot: ReadinessSnapshot): ReadinessStepView {
  if (snapshot.entryLoadFailed) return { state: "attention", value: t("overview.loadFailed") };
  if (snapshot.entryLoading) return { state: "pending", value: t("overview.checking") };
  if (snapshot.modelCount === 0) return { state: "pending", value: t("overview.step1Unconfigured") };
  if (!snapshot.proxyRunning) return { state: "pending", value: t("overview.step2Stopped") };
  return snapshot.readyClientCount > 0
    ? {
        state: "ready",
        value: t("overview.step3Ready", { count: snapshot.readyClientCount }),
      }
    : { state: "attention", value: `${t("overview.step3Action")} →` };
}

function readinessHeader(snapshot: ReadinessSnapshot): { title: string; detail: string } {
  if (snapshot.modelCount === 0) {
    return { title: t("overview.step1HeaderTitle"), detail: t("overview.step1HeaderDesc") };
  }
  if (snapshot.proxyLoadFailed || snapshot.entryLoadFailed) {
    return { title: t("overview.loadFailed"), detail: t("overview.loadFailed") };
  }
  if (snapshot.proxyLoading || snapshot.entryLoading) {
    return { title: t("overview.checkingStatusTitle"), detail: t("overview.checkingStatusDetail") };
  }
  if (!snapshot.proxyRunning) {
    return { title: t("overview.step2HeaderTitle"), detail: t("overview.step2HeaderDesc") };
  }
  if (snapshot.readyClientCount === 0) {
    return { title: t("overview.step3HeaderTitle"), detail: t("overview.step3HeaderDesc") };
  }
  return { title: t("overview.step4HeaderTitle"), detail: t("overview.step4HeaderDesc") };
}

export function renderReadiness(): void {
  const snapshot = readinessSnapshot();

  setReadinessStep(
    "#readiness-models",
    "#readiness-models-value",
    modelsStep(snapshot),
  );
  setReadinessStep(
    "#readiness-proxy",
    "#readiness-proxy-value",
    proxyStep(snapshot),
  );
  setReadinessStep(
    "#readiness-entry",
    "#readiness-entry-value",
    entryStep(snapshot),
  );
  setReadinessStep(
    "#readiness-restore",
    "#readiness-restore-value",
    { state: "ready", value: t("overview.step4Value") },
  );

  const header = readinessHeader(snapshot);
  const title = element<HTMLHeadingElement>("#readiness-title");
  const detail = element<HTMLParagraphElement>("#readiness-detail");
  title.textContent = header.title;
  detail.textContent = header.detail;
}

export function setupReadinessPanel(): void {
  const modelsStep = document.querySelector<HTMLElement>("#readiness-models");
  const proxyStep = document.querySelector<HTMLElement>("#readiness-proxy");
  const entryStep = document.querySelector<HTMLElement>("#readiness-entry");

  if (modelsStep) {
    modelsStep.addEventListener("click", () => {
      void switchTab("tab-models");
    });
  }

  if (proxyStep) {
    proxyStep.addEventListener("click", () => {
      const modelCount = store.config.virtual_models.length;
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
    entryStep.addEventListener("click", () => {
      const modelCount = store.config.virtual_models.length;
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
