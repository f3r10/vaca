/** Hand a string to the browser as a downloaded file. Nothing is stored. */
export function downloadText(filename: string, contents: string): void {
  const url = URL.createObjectURL(new Blob([contents], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
