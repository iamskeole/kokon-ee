// The lab's segmenter selection (sam.ts, task-037 P3 / D-037-13): the model never runs in the
// browser. The box runs it — its stock GGUF door when the door answers (a split box), its own
// pinned ONNX files when it has them (a single box, and a mixed box's escape when its door is
// down); a single box without the files downloads them on first use and runs them in the same
// session; a thin box whose *named* door is not answering gets `null` — said plainly, never a
// download onto a box that is not the model's home.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ServiceSegmenter, loadSegmenter } from "./sam";

type Status = { installed: boolean; bytes: number; doorConfigured: boolean; door: boolean };

let mockStatuses: Array<Status | null> = [];
let mockStatusDefault: Status | null = null;
let mockDownloadCalls = 0;
let mockInstallFailed = false;

const st = (installed: boolean, doorConfigured = false, door = false): Status => ({
  installed,
  bytes: 45_000_000,
  doorConfigured,
  door,
});

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
  urls: {},
}));

beforeEach(() => {
  mockStatuses = [];
  mockStatusDefault = null;
  mockDownloadCalls = 0;
  mockInstallFailed = false;
});

afterEach(() => vi.unstubAllGlobals());

describe("loadSegmenter", () => {
  it("a split box with an answering door: the door, no download", async () => {
    mockStatusDefault = st(false, true, true);
    const s = await loadSegmenter("input:x");
    expect(s).toBeInstanceOf(ServiceSegmenter);
    expect(mockDownloadCalls).toBe(0);
  });

  it("a single box with the files: in-process, no download", async () => {
    mockStatusDefault = st(true);
    const s = await loadSegmenter("input:x");
    expect(s).toBeInstanceOf(ServiceSegmenter);
    expect(mockDownloadCalls).toBe(0);
  });

  it("first use on a single box without the files: downloads, then runs in the same session", async () => {
    // the initial check (not there), then the re-check after the download (there)
    mockStatuses = [st(false), st(true)];
    const s = await loadSegmenter("input:x");
    expect(s).toBeInstanceOf(ServiceSegmenter);
    expect(mockDownloadCalls).toBe(1);
  }, 10_000);

  it("a failed single-box download is the lab's 'missing' state", async () => {
    mockStatuses = [st(false), st(false)];
    mockInstallFailed = true;
    expect(await loadSegmenter("input:x")).toBeNull();
    expect(mockDownloadCalls).toBe(1);
  }, 10_000);

  it("a thin box whose named door is not answering: null, plainly, and no download onto the thin box", async () => {
    mockStatusDefault = st(false, true, false);
    expect(await loadSegmenter("input:x")).toBeNull();
    expect(mockDownloadCalls).toBe(0);
  });

  it("no service at all: null (there is no in-browser copy to fall back on)", async () => {
    expect(await loadSegmenter("input:x")).toBeNull();
    expect(mockDownloadCalls).toBe(0);
  });

  it("no source image (no service id to ask with): null", async () => {
    mockStatusDefault = st(true);
    expect(await loadSegmenter(null)).toBeNull();
    expect(mockDownloadCalls).toBe(0);
  });
});
