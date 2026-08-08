import { refreshApp, refreshCli, refreshIde } from "../../controllers/hostController";
import { setProxyPort } from "../../controllers/proxyController";
import { store } from "../../store/appStore";
import { MIN_PROXY_PORT } from "../../types/config";
import { errorMessage } from "../../utils/errorUtils";
import { t } from "../../i18n";
import { showNotice } from "../NoticeBar";

function validProxyPort(value: number): boolean {
  return Number.isInteger(value) && value >= MIN_PROXY_PORT && value <= 65535;
}

export function setupProxyPortSettings(): void {
  const saveButton = document.querySelector<HTMLButtonElement>("#save-proxy-port");
  const input = document.querySelector<HTMLInputElement>("#settings-proxy-port");
  if (!saveButton || !input) return;
  let saveInProgress = false;

  const render = () => {
    const currentPort = Number(input.value.trim());
    const configAvailable = store.configLoaded;
    input.disabled = !configAvailable || saveInProgress;
    saveButton.disabled = saveInProgress
      || !configAvailable
      || currentPort === store.config.proxy_port
      || !validProxyPort(currentPort);
  };
  const save = async () => {
    if (saveInProgress) return;
    if (!store.configLoaded) {
      showNotice(store.configLoadError ?? t("overview.loadFailed"), "error");
      return;
    }
    const newPort = Number(input.value.trim());
    if (!validProxyPort(newPort)) {
      showNotice(t("settings.invalidPort"), "error");
      return;
    }

    const proxyWasRunning = store.proxyStatus?.state === "running";
    saveInProgress = true;
    render();
    try {
      if (proxyWasRunning) showNotice(t("settings.restartingProxy", { port: newPort }));
      const { port: savedPort } = await setProxyPort(newPort);
      input.value = String(savedPort);
      const refreshResults = await Promise.allSettled([refreshIde(), refreshApp(), refreshCli()]);
      render();
      if (refreshResults.some((result) => result.status === "rejected")) {
        showNotice(t("settings.portSavedHostRefreshFailed", { port: savedPort }), "error");
      } else {
        showNotice(
          t(proxyWasRunning ? "settings.portSavedRunning" : "settings.portSavedStopped", {
            port: savedPort,
          }),
          "success",
        );
      }
    } catch (error) {
      showNotice(t("settings.portSaveFailed", { message: errorMessage(error) }), "error");
    } finally {
      saveInProgress = false;
      render();
    }
  };

  input.value = String(store.config.proxy_port);
  render();
  store.subscribeConfig(() => {
    if (document.activeElement !== input) input.value = String(store.config.proxy_port);
    render();
  });
  input.addEventListener("input", render);
  input.addEventListener("change", render);
  input.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || saveButton.disabled) return;
    event.preventDefault();
    void save();
  });
  saveButton.addEventListener("click", () => void save());
}
