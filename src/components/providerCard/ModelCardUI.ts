import { t } from "../../i18n";

export interface ModelCardUIProps {
  titleNode: HTMLElement;
  capabilitiesNode: HTMLElement;
  variantsNode: HTMLElement;
  policyNode: HTMLElement;
}

export function buildModelCardUI(props: ModelCardUIProps): HTMLElement {
  const item = document.createElement("article");
  item.className = "provider-model-item";

  // --- Header ---
  const header = document.createElement("header");
  header.className = "model-card-header";
  header.append(props.titleNode, props.capabilitiesNode);

  // --- Body ---
  const body = document.createElement("div");
  body.className = "model-card-body";
  
  const bodyLabel = document.createElement("div");
  bodyLabel.className = "model-card-section-title";
  bodyLabel.textContent = t("models.variants");
  
  body.append(bodyLabel, props.variantsNode);

  // --- Footer ---
  const footer = document.createElement("footer");
  footer.className = "model-card-footer";
  
  const footerLabel = document.createElement("div");
  footerLabel.className = "model-card-section-title";
  footerLabel.textContent = t("models.policy");

  footer.append(footerLabel, props.policyNode);

  item.append(header, body, footer);
  return item;
}
