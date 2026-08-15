import { t } from "../../i18n";
import type { ModelCompressionPolicy } from "../../types/config";
import {
  formatTokenCount,
  matchingPreset,
  presetLabel,
} from "./policyPresets";

export function createMetric(label: string, value: number, subtext?: string): HTMLDivElement {
  const metric = document.createElement("div");
  metric.className = "policy-metric";

  const name = document.createElement("span");
  name.textContent = label;

  const valueRow = document.createElement("div");
  const count = document.createElement("strong");
  count.textContent = value.toLocaleString();
  valueRow.append(count);

  if (subtext) {
    const badge = document.createElement("small");
    badge.textContent = subtext;
    valueRow.append(badge);
  }

  metric.append(name, valueRow);
  return metric;
}

export function renderCapacityBar(
  container: HTMLElement,
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
): void {
  if (!policy || !capacity || capacity <= 0) return;

  const threshold = policy.token_threshold;
  const limit = policy.max_token_limit;

  const thresholdPct = Math.min(100, Math.max(0, (threshold / capacity) * 100));
  const limitPct = Math.min(100, Math.max(0, (limit / capacity) * 100));
  const compressPct = Math.max(0, limitPct - thresholdPct);
  const reservePct = Math.max(0, 100 - limitPct);

  const wrapper = document.createElement("div");
  wrapper.className = "policy-capacity-bar-wrapper";

  const labelRow = document.createElement("div");
  labelRow.className = "policy-capacity-bar-labels";

  const title = document.createElement("span");
  title.className = "policy-bar-title";
  title.textContent = t("models.policyCapacityBarTitle");

  const legend = document.createElement("div");
  legend.className = "policy-bar-legend";

  const chatLegend = document.createElement("span");
  chatLegend.className = "legend-item legend-chat";
  chatLegend.textContent = t("models.policyContextChatSegment");

  const compressLegend = document.createElement("span");
  compressLegend.className = "legend-item legend-compress";
  compressLegend.textContent = t("models.policyContextCompressSegment");

  const reserveLegend = document.createElement("span");
  reserveLegend.className = "legend-item legend-reserve";
  reserveLegend.textContent = t("models.policyContextReserveSegment");

  legend.append(chatLegend, compressLegend, reserveLegend);
  labelRow.append(title, legend);

  const bar = document.createElement("div");
  bar.className = "policy-capacity-bar";

  const chatSegment = document.createElement("div");
  chatSegment.className = "bar-segment segment-chat";
  chatSegment.style.width = `${thresholdPct.toFixed(2)}%`;
  chatSegment.title = `${t("models.policyContextChatSegment")}: 0 ～ ${formatTokenCount(threshold)}`;

  const compressSegment = document.createElement("div");
  compressSegment.className = "bar-segment segment-compress";
  compressSegment.style.width = `${compressPct.toFixed(2)}%`;
  compressSegment.title = `${t("models.policyContextCompressSegment")}: ${formatTokenCount(threshold)} ～ ${formatTokenCount(limit)}`;

  const reserveSegment = document.createElement("div");
  reserveSegment.className = "bar-segment segment-reserve";
  reserveSegment.style.width = `${reservePct.toFixed(2)}%`;
  reserveSegment.title = `${t("models.policyContextReserveSegment")}: ${formatTokenCount(limit)} ～ ${formatTokenCount(capacity)}`;

  bar.append(chatSegment, compressSegment, reserveSegment);

  const ticksRow = document.createElement("div");
  ticksRow.className = "policy-capacity-ticks";

  const tickStart = document.createElement("span");
  tickStart.textContent = "0";

  const tickTrigger = document.createElement("span");
  tickTrigger.className = "tick-trigger";
  tickTrigger.textContent = `${formatTokenCount(threshold)}`;

  const tickLimit = document.createElement("span");
  tickLimit.className = "tick-limit";
  tickLimit.textContent = `${formatTokenCount(limit)}`;

  const tickCap = document.createElement("span");
  tickCap.className = "tick-cap";
  tickCap.textContent = `${formatTokenCount(capacity)}`;

  ticksRow.append(tickStart, tickTrigger, tickLimit, tickCap);

  wrapper.append(labelRow, bar, ticksRow);
  container.append(wrapper);
}

export function renderPolicyMetrics(
  container: HTMLElement,
  policy: ModelCompressionPolicy,
  capacity: number | null,
): void {
  const metrics = document.createElement("div");
  metrics.className = "policy-metric-grid";

  metrics.append(
    createMetric(t("models.policyThreshold"), policy.token_threshold, formatTokenCount(policy.token_threshold)),
    createMetric(t("models.policyMaxLimit"), policy.max_token_limit, formatTokenCount(policy.max_token_limit)),
    createMetric(t("models.policyMaxOutput"), policy.max_output_tokens, formatTokenCount(policy.max_output_tokens)),
  );
  container.append(metrics);
  renderCapacityBar(container, policy, capacity);
}

export function getPolicyPillStatus(
  policy: ModelCompressionPolicy | null,
  capacity: number | null,
  outputTokenLimit: number | null,
  defaultLabel: string,
): { label: string; isManaged: boolean; tooltip: string } {
  if (!policy || !policy.enabled) {
    return {
      label: defaultLabel,
      isManaged: false,
      tooltip: t("models.policyPillTooltipDefault", { label: defaultLabel }),
    };
  }
  const preset = matchingPreset(policy, capacity, outputTokenLimit);
  const label = preset ? presetLabel(preset) : t("models.presetCustom");
  return {
    label,
    isManaged: true,
    tooltip: t("models.policyPillTooltip", {
      label,
      threshold: formatTokenCount(policy.token_threshold),
      limit: formatTokenCount(policy.max_token_limit),
      output: formatTokenCount(policy.max_output_tokens),
    }),
  };
}
