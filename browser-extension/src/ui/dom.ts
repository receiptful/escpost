import type { PopupView } from "../popup/model";

export function renderPopup(main: HTMLElement, view: PopupView, onPrimaryAction: () => void): void {
  const heading = element("h1", "ESCPost");
  heading.id = "popup-title";
  const description = element("p", "Raw printing extension");
  description.className = "lede";

  const statuses = element("dl");
  statuses.className = "statuses";
  statuses.append(
    statusRow("Current site", view.origin, "current-origin"),
    statusRow("Site access", view.permission.label, "permission-status", view.permission.tone),
    statusRow("Daemon", view.daemon.label, "daemon-status", view.daemon.tone),
  );

  const guidance = element("p", view.guidance);
  guidance.id = "reload-guidance";
  guidance.className = "guidance";
  guidance.setAttribute("role", "status");

  const error = element("p", view.error ?? "");
  error.id = "popup-error";
  error.className = "error";
  error.setAttribute("role", "alert");
  error.hidden = view.error === null;

  const children: Node[] = [heading, description, statuses, guidance, error];
  if (view.primaryAction !== null) {
    const action = document.createElement("button");
    action.id = "permission-action";
    action.type = "button";
    action.textContent = view.primaryAction.label;
    action.addEventListener("click", onPrimaryAction);
    children.push(action);
  }
  main.replaceChildren(...children);
}

function statusRow(label: string, value: string, valueId: string, tone?: string): DocumentFragment {
  const row = document.createDocumentFragment();
  row.append(element("dt", label));
  const detail = element("dd", value);
  detail.id = valueId;
  if (tone !== undefined) detail.dataset.tone = tone;
  row.append(detail);
  return row;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const result = document.createElement(tag);
  if (text !== undefined) result.textContent = text;
  return result;
}
