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
