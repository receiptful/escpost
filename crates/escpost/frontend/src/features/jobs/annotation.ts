import { copyText } from "../../app/clipboard";

export function webUrl(content: string): string | null {
  if (!content.startsWith("http://") && !content.startsWith("https://")) return null;
  try {
    const url = new URL(content);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

export function activateAnnotation(content: string) {
  void copyText(content);
  const href = webUrl(content);
  if (href) window.open(href, "_blank", "noopener,noreferrer");
}
