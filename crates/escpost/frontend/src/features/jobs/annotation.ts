export function webUrl(content: string): string | null {
  if (!content.startsWith("http://") && !content.startsWith("https://")) return null;
  try {
    const url = new URL(content);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

export async function copyText(text: string) {
  try {
    await navigator.clipboard?.writeText(text);
    if (navigator.clipboard) return;
  } catch {
    // Fall through to the browser-compatible textarea path.
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.append(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

export function activateAnnotation(content: string) {
  void copyText(content);
  const href = webUrl(content);
  if (href) window.open(href, "_blank", "noopener,noreferrer");
}
