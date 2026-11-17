import { describe, expect, it } from "vitest";
import { freshFailure } from "./store";
import type { AppErrorBody, JobStatus, JobView } from "./types";

const NOW = Date.now();
const iso = (msAgo: number) => new Date(NOW - msAgo).toISOString();

const ERR: AppErrorBody = { code: "server_unavailable", message: "The inference server did not report its capabilities.", detail: "HTTP 500 Internal Server Error" };

const job = (status: JobStatus, finishedAt: number | null = 5_000): JobView => ({
  id: "j1",
  status,
  stage: null,
  step: null,
  image: null,
  seconds_per_step: null,
  queue_position: 0,
  created_at: iso(60_000),
  started_at: iso(30_000),
  finished_at: finishedAt === null ? null : iso(finishedAt),
  prompt: "a fox",
  plan: null,
  seed: null,
  results: [],
  error: status === "failed" ? ERR : null,
  notes: [],
  detailed_progress: false,
});

/** The 2026-11-17 w030 incident: two jobs that failed hours earlier kept toasting on every
 * reconnect, because the catch-up re-lists every job and the toast had no notion of news. */
describe("freshFailure: the toast is for news, not for history", () => {
  it("toasts when the client watched the job go from live to failed", () => {
    expect(freshFailure(job("running", null), job("failed", 1_000))).toBe(true);
    expect(freshFailure(job("queued", null), job("failed", 1_000))).toBe(true);
  });

  it("toasts a first sighting that failed within the last minute (it may have run while the tab was closed)", () => {
    expect(freshFailure(undefined, job("failed", 30_000))).toBe(true);
  });

  it("does not toast a first sighting of an old failure (the w030 fossils)", () => {
    expect(freshFailure(undefined, job("failed", 3 * 3600_000))).toBe(false);
    expect(freshFailure(undefined, job("failed", 61_000))).toBe(false);
  });

  it("does not re-toast a failure the client already saw (the reconnect catch-up re-lists it)", () => {
    expect(freshFailure(job("failed", 1_000), job("failed", 1_000))).toBe(false);
    expect(freshFailure(job("failed", 5 * 60_000), job("failed", 5 * 60_000))).toBe(false);
  });

  it("ignores jobs that are not failures", () => {
    expect(freshFailure(job("running", null), job("done", 1_000))).toBe(false);
    expect(freshFailure(undefined, job("done", 1_000))).toBe(false);
    expect(freshFailure(job("queued", null), job("cancelled", 1_000))).toBe(false);
  });

  it("ignores a failure with no finished_at (no way to tell how stale it is)", () => {
    expect(freshFailure(undefined, job("failed", null))).toBe(false);
    expect(freshFailure(job("running", null), job("failed", null))).toBe(false);
  });
});
