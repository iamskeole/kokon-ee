// The lab's segmenter selection (sam.ts): the box runs MobileSAM in-process whenever it can —
// a fileless box downloads the pinned files on first use and runs them in the *same* session;
// the in-browser onnxruntime-web copy is the last resort (no reachable service, or the box's
// download failed and no dev-server copy either).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ServiceSegmenter, loadSegmenter } from "./sam";

let mockStatuses: Array<{ installed: boolean } | null> = [];
let mockStatusDefault: { installed: boolean } | null = null;
let mockDownloadCalls = 0;
let mockInstallFailed = false;
let mockDevDirOk = false;
let mockOrtCreates = 0;

vi.mock("./api", () => ({
  api: {
    segmentStatus: async () => mockStatuses.shift() ?? mockStatusDefault,
    downloadSegment: async () => {
      mockDownloadCalls += 1;
      return { running: true };
    },
    installStatus: async () => ({
      running: false,
      items: [
        {
          id: "mobile_sam_encoder",
          state: mockInstallFailed ? "failed" : "done",
          size: 1,
          received: 1,
        },
      ],
    }),
  },
  urls: { segmentModel: () => "http://127.0.0.1/segment/models/" },
}));

vi.mock("onnxruntime-web", () => ({
  env: { logLevel: "info" },
  InferenceSession: {
    create: async () => {
      mockOrtCreates += 1;
      throw new Error("no ONNX in the test");
    },
  },
}));

beforeEach(() => {
  mockStatuses = [];
  mockStatusDefault = null;
  mockDownloadCalls = 0;
  mockInstallFailed = false;
  mockDevDirOk = false;
  mockOrtCreates = 0;
  vi.stubGlobal(
    "fetch",
    async (_url: string, init?: { method?: string }) =>
      init?.method === "HEAD"
        ? { ok: mockDevDirOk, headers: { get: () => (mockDevDirOk ? "application/octet-stream" : "text/html") } }
        : { ok: false },
  );
});

afterEach(() => vi.unstubAllGlobals());

describe("loadSegmenter", () => {
  it("uses the service's model when the box has the files (no download)", async () => {
    mockStatusDefault = { installed: true };
    const s = await loadSegmenter("input:x");
    expect(s).toBeInstanceOf(ServiceSegmenter);
    expect(mockDownloadCalls).toBe(0);
  });

  it("first use on a fileless box: downloads, then runs in-process in the same session", async () => {
    // the initial check (not there), then the re-check after the download (there)
    mockStatuses = [{ installed: false }, { installed: true }];
    const s = await loadSegmenter("input:x");
    expect(s).toBeInstanceOf(ServiceSegmenter);
    expect(mockDownloadCalls).toBe(1);
  }, 10_000);

  it("a failed box download falls through to the in-browser copy when the dev server has it", async () => {
    mockStatuses = [{ installed: false }, { installed: false }, { installed: false }, { installed: false }];
    mockInstallFailed = true;
    mockDevDirOk = true;
    await expect(loadSegmenter("input:x")).rejects.toThrow("no ONNX in the test");
    expect(mockDownloadCalls).toBe(2); // loadSegmenter's attempt, then the in-browser path's retry
    expect(mockOrtCreates).toBeGreaterThan(0);
  }, 10_000);

  it("a failed box download with no dev copy is the lab's 'missing' state", async () => {
    mockStatuses = [{ installed: false }, { installed: false }, { installed: false }, { installed: false }];
    mockInstallFailed = true;
    expect(await loadSegmenter("input:x")).toBeNull();
    expect(mockDownloadCalls).toBe(2);
    expect(mockOrtCreates).toBe(0);
  }, 10_000);

  it("no reachable service at all: the in-browser copy (the stock path), no download", async () => {
    mockDevDirOk = true;
    await expect(loadSegmenter("input:x")).rejects.toThrow("no ONNX in the test");
    expect(mockDownloadCalls).toBe(0);
    expect(mockOrtCreates).toBeGreaterThan(0);
  }, 10_000);

  it("a canvas source (not in the service) always runs in the browser", async () => {
    mockStatusDefault = { installed: true };
    mockDevDirOk = true;
    await expect(loadSegmenter(null)).rejects.toThrow("no ONNX in the test");
    expect(mockDownloadCalls).toBe(0);
  }, 10_000);
});
