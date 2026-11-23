// Native integrations. The Tauri desktop shell is gone (task-031): the product is the browser
// UI served by the box service, so every function here is plain browser behaviour. A few
// desktop-only signatures survive, degraded, for code paths the cut removes in a later phase.

export interface Connection { baseUrl: string; token: string; version: string }

/** Copying images in a browser needs a secure page (https, e.g. through Tailscale, or localhost). */
export const canCopyImage = typeof window !== "undefined" && window.isSecureContext && !!navigator.clipboard?.write && typeof ClipboardItem !== "undefined";

/** The browser's clipboard takes PNG only. */
async function asPng(blob: Blob): Promise<Blob> {
  if (blob.type === "image/png") return blob;
  const bmp = await createImageBitmap(blob);
  const canvas = document.createElement("canvas");
  canvas.width = bmp.width;
  canvas.height = bmp.height;
  canvas.getContext("2d")!.drawImage(bmp, 0, 0);
  return new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("Could not copy the image."))), "image/png"));
}

export const native = {
  /** The API is on the page's own server, signed in by cookie. */
  connect: () => Promise.resolve<Connection>({ baseUrl: "/api/v1", token: "", version: "" }),
  /** Copying an image by local path had a native counterpart; the box serves every image by URL. */
  copyImage: (_path: string) => Promise.reject(new Error("Images are copied from their URL on this box.")),
  copyImageUrl: async (url: string) => {
    const res = await fetch(url);
    if (!res.ok) throw new Error("Could not read the image.");
    const png = asPng(await res.blob());
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
  },
  /** Text to the clipboard. A page on plain http (the LAN) has no clipboard API: the fallback
   * still works there. */
  copyText: async (text: string) => {
    if (window.isSecureContext && navigator.clipboard?.writeText) return navigator.clipboard.writeText(text);
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    if (!ok) throw new Error("Could not copy the text.");
  },
  /** The browser's clipboard cannot hand an image back to the page (no read API on the web). */
  readClipboardImage: async () => null,
  reveal: (_path: string) => Promise.resolve(),
  openFolder: (_path: string) => Promise.resolve(),
  dataFolder: async () => "",
  openLink: async (url: string) => window.open(url, "_blank", "noopener,noreferrer"),
  pickImages: async () => [],
  /** The images picked on this device, to upload. The input stays in the page until it answers:
   * iOS Safari drops a detached one while the Camera is open (Take Photo never came back; the
   * photo library, quicker, did). */
  pickImageFiles: (): Promise<File[]> =>
    new Promise((resolve) => {
      const input = document.createElement("input");
      input.type = "file";
      input.multiple = true;
      input.accept = "image/*,.heic,.heif";
      input.hidden = true;
      const done = (files: File[]) => {
        input.remove();
        resolve(files);
      };
      input.onchange = () => done(Array.from(input.files ?? []));
      input.oncancel = () => done([]);
      document.body.appendChild(input);
      input.click();
    }),
  /** Native file picking had a desktop counterpart; the browser uploads through its own inputs. */
  pickFile: async (_extensions?: string[], _defaultPath?: string, _kind?: string) => null,
  pickFolder: async (_defaultPath?: string) => null,
  confirm: (message: string, _title?: string) => Promise.resolve(window.confirm(message)),
  isFullscreen: async () => !!document.fullscreenElement,
  setFullscreen: async (on: boolean) => {
    if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen?.();
    else if (!on && document.fullscreenElement) await document.exitFullscreen();
  },
  setTheme: async (_theme?: "light" | "dark" | null) => undefined,
  onResized: async (f: () => void) => {
    window.addEventListener("resize", f);
    return () => window.removeEventListener("resize", f);
  },
  saveAs: async (_defaultName?: string) => null,
  /** Saves the file at `url` on this device (the browser's downloads). */
  download: (url: string, name: string) => {
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
  },
  /** The Layers lab's .psd: the browser downloads it. */
  savePsd: async (psd: Uint8Array) => {
    const url = URL.createObjectURL(new Blob([psd as BlobPart], { type: "image/vnd.adobe.photoshop" }));
    native.download(url, "Layers.psd");
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
    return "Layers.psd";
  },
};
