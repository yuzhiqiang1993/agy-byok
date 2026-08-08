import { t } from "../../i18n";
import { store } from "../../store/appStore";
import type {
  ClientConfigurationState,
  ClientIntegrationState,
} from "../../types/host";
import { element, withBusy, withClientBusy } from "../../utils/domUtils";
import { confirmHostAction } from "../ConfirmModal";
import { showNotice } from "../NoticeBar";
import { switchTab } from "../TabManager";

type HostClient = "ide" | "app" | "cli";
type ActionMessages = "desktop" | "cli";

interface IntegrationStatus {
  integrationState: ClientIntegrationState;
  configurationState: ClientConfigurationState;
}

interface HostIntegrationActions<S extends IntegrationStatus> {
  client: HostClient;
  messages: ActionMessages;
  getCurrentStatus: () => S | null;
  isRunning: (status: S | null) => boolean;
  enable: () => Promise<S>;
  disable: () => Promise<S>;
  refresh: () => Promise<void>;
  render: (status: S) => void;
  launch?: () => Promise<void>;
  integrationRemainsActiveAfterDisable?: (status: S) => boolean;
}

interface EnableContext {
  clientLabel: string;
  isRunning: boolean;
  needsReconfiguration: boolean;
  alreadyEnabled: boolean;
}

function clientLabel(client: HostClient): string {
  if (client === "ide") return t("overview.clientIde");
  if (client === "app") return t("overview.clientApp");
  return t("overview.clientCli");
}

function enableConfirmationMessage(messages: ActionMessages, context: EnableContext): string {
  if (messages === "cli") {
    if (context.needsReconfiguration) return t("overview.cliUpdateConfirm");
    return t(context.alreadyEnabled ? "overview.cliAlreadyEnabledConfirm" : "overview.cliEnableConfirm");
  }
  if (context.needsReconfiguration) {
    return t(
      context.isRunning ? "overview.hostUpdateConfirmRunning" : "overview.hostUpdateConfirmStopped",
      { client: context.clientLabel },
    );
  }
  if (context.alreadyEnabled) {
    return t("overview.hostAlreadyEnabledConfirm", { client: context.clientLabel });
  }
  return t(
    context.isRunning ? "overview.hostEnableConfirmRunning" : "overview.hostEnableConfirmStopped",
    { client: context.clientLabel },
  );
}

function enableSuccessMessage<S extends IntegrationStatus>(
  spec: HostIntegrationActions<S>,
  context: EnableContext,
  status: S,
): string {
  if (spec.messages === "cli") {
    if (context.alreadyEnabled && status.integrationState === "managed") {
      return t("overview.cliAlreadyEnabled");
    }
    return t(context.needsReconfiguration ? "overview.cliUpdated" : "overview.cliEnabled");
  }
  const stillEnabled = status.integrationState === "managed"
    && status.configurationState !== "needs_update";
  if (context.alreadyEnabled && stillEnabled) {
    return t("overview.hostAlreadyEnabled", { client: context.clientLabel });
  }
  if (context.needsReconfiguration) {
    return t(
      spec.isRunning(status) ? "overview.hostUpdatedRunning" : "overview.hostUpdatedStopped",
      { client: context.clientLabel },
    );
  }
  return t(
    spec.isRunning(status) ? "overview.hostEnabledRunning" : "overview.hostEnabledStopped",
    { client: context.clientLabel },
  );
}

async function refreshAfterFailedAction<S extends IntegrationStatus>(
  spec: HostIntegrationActions<S>,
): Promise<void> {
  if (!spec.getCurrentStatus()) return;
  try {
    await spec.refresh();
  } catch {
    // withClientBusy 已展示原始操作错误；刷新只用于恢复最新状态。
  }
}

function bindEnableAction<S extends IntegrationStatus>(
  button: HTMLButtonElement,
  spec: HostIntegrationActions<S>,
): void {
  button.addEventListener("click", () => {
    void (async () => {
      if (store.config.virtual_models.length === 0) {
        showNotice(t("overview.hostModelsRequired", { count: 1 }), "error");
        void switchTab("tab-models");
        return;
      }
      const current = spec.getCurrentStatus();
      const context: EnableContext = {
        clientLabel: clientLabel(spec.client),
        isRunning: spec.isRunning(current),
        needsReconfiguration: current?.integrationState === "mismatch"
          || current?.configurationState === "needs_update",
        alreadyEnabled: current?.integrationState === "managed"
          && current.configurationState !== "needs_update",
      };
      const status = await withClientBusy(button, spec.client, async () => {
        const confirmed = await confirmHostAction(
          enableConfirmationMessage(spec.messages, context),
          t(context.needsReconfiguration ? "overview.hostUpdateTitle" : "overview.hostEnableTitle"),
          t(context.needsReconfiguration ? "overview.hostUpdateOk" : "overview.hostEnableOk"),
          t("overview.hostCancel"),
        );
        if (!confirmed) return null;
        showNotice(t(
          context.needsReconfiguration ? "overview.hostUpdating" : "overview.hostEnabling",
          { client: context.clientLabel },
        ));
        return spec.enable();
      }, showNotice);
      if (status === null) return;
      if (status) {
        spec.render(status);
        showNotice(enableSuccessMessage(spec, context, status));
      } else {
        await refreshAfterFailedAction(spec);
      }
    })();
  });
}

function bindDisableAction<S extends IntegrationStatus>(
  button: HTMLButtonElement,
  spec: HostIntegrationActions<S>,
): void {
  button.addEventListener("click", () => {
    void (async () => {
      const current = spec.getCurrentStatus();
      const label = clientLabel(spec.client);
      const status = await withClientBusy(button, spec.client, async () => {
        const message = spec.messages === "cli"
          ? t("overview.cliDisableConfirm")
          : t(
              spec.isRunning(current)
                ? "overview.hostDisableConfirmRunning"
                : "overview.hostDisableConfirmStopped",
              { client: label },
            );
        const confirmed = await confirmHostAction(
          message,
          t("overview.hostDisableTitle"),
          t("overview.hostDisableOk"),
          t("overview.hostCancel"),
        );
        if (!confirmed) return null;
        showNotice(t("overview.hostDisabling", { client: label }));
        return spec.disable();
      }, showNotice);
      if (status === null) return;
      if (status) {
        spec.render(status);
        if (spec.integrationRemainsActiveAfterDisable?.(status)) {
          showNotice(t("overview.hostIntegrationStillActive", { client: label }));
        } else if (spec.messages === "cli") {
          showNotice(t("overview.cliDisabled"));
        } else {
          showNotice(t(
            spec.isRunning(status) ? "overview.hostDisabledRunning" : "overview.hostDisabledStopped",
            { client: label },
          ));
        }
      } else {
        await refreshAfterFailedAction(spec);
      }
    })();
  });
}

function bindLaunchAction<S extends IntegrationStatus>(
  button: HTMLButtonElement,
  spec: HostIntegrationActions<S>,
): void {
  if (!spec.launch) return;
  button.addEventListener("click", () => {
    void withClientBusy(button, spec.client, async () => {
      await spec.launch?.();
      showNotice(t("overview.hostLaunched", { client: clientLabel(spec.client) }));
      window.setTimeout(() => void spec.refresh().catch(() => undefined), 700);
    }, showNotice, t("overview.hostLaunching", { client: clientLabel(spec.client) }));
  });
}

export function setupHostIntegrationActions<S extends IntegrationStatus>(
  spec: HostIntegrationActions<S>,
): void {
  bindEnableAction(element<HTMLButtonElement>(`#enable-${spec.client}-integration`), spec);
  bindDisableAction(element<HTMLButtonElement>(`#disable-${spec.client}-integration`), spec);
  element<HTMLButtonElement>(`#refresh-${spec.client}`).addEventListener("click", (event) => {
    void withBusy(event.currentTarget as HTMLButtonElement, spec.refresh, showNotice);
  });
  if (spec.launch) bindLaunchAction(element<HTMLButtonElement>(`#launch-${spec.client}`), spec);
}
