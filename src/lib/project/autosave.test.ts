import { describe, expect, it, vi } from "vitest";
import { AutosaveController } from "./autosave";

describe("AutosaveController", () => {
  it("coalesces edits and persists an immutable snapshot", async () => {
    const writes: Array<{ value: number; reason: string }> = [];
    const controller = new AutosaveController({
      write: async (document: { value: number }, reason) => {
        writes.push({ value: document.value, reason });
      },
    });
    const document = { value: 1 };

    controller.markDirty(document);
    document.value = 2;
    controller.markDirty(document);
    document.value = 3;
    await controller.flush("test");

    expect(writes).toEqual([{ value: 2, reason: "test" }]);
    expect(controller.getStatus().savedRevision).toBe(2);
    await controller.dispose();
  });

  it("clones proxy-backed JSON state without structuredClone errors", async () => {
    const writes: Array<{ captions: string[] }> = [];
    const controller = new AutosaveController({
      write: async (document: { captions: string[] }) => {
        writes.push(document);
      },
    });
    const captions = new Proxy(["Merhaba"], {});
    const reactiveDocument = new Proxy({ captions }, {});

    controller.markDirty(reactiveDocument);
    captions.push("sonradan");
    await controller.flush("proxy-regression");

    expect(writes).toEqual([{ captions: ["Merhaba"] }]);
    await controller.dispose();
  });

  it("serializes writes and does not lose changes made during a save", async () => {
    let releaseFirst!: () => void;
    const firstPending = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const writes: number[] = [];
    const controller = new AutosaveController({
      write: async (document: { value: number }) => {
        writes.push(document.value);
        if (writes.length === 1) await firstPending;
      },
    });

    controller.markDirty({ value: 1 });
    const firstFlush = controller.flush("first");
    await vi.waitFor(() => expect(writes).toEqual([1]));
    controller.markDirty({ value: 2 });
    releaseFirst();
    await firstFlush;

    expect(writes).toEqual([1, 2]);
    expect(controller.getStatus().savedRevision).toBe(2);
    await controller.dispose();
  });

  it("retains the failed revision so a manual retry can recover", async () => {
    let attempts = 0;
    const controller = new AutosaveController({
      write: async () => {
        attempts += 1;
        if (attempts === 1) throw new Error("disk full");
      },
    });

    controller.markDirty({ value: 1 });
    await expect(controller.flush("first")).rejects.toThrow("disk full");
    expect(controller.getStatus().phase).toBe("error");
    await controller.flush("retry");

    expect(attempts).toBe(2);
    expect(controller.getStatus().phase).toBe("saved");
    await controller.dispose();
  });

  it("acknowledges only the project revision that was durably committed", async () => {
    const writes: number[] = [];
    const controller = new AutosaveController({
      delayMs: 60_000,
      maxWaitMs: 60_000,
      write: async (document: { value: number }) => {
        writes.push(document.value);
      },
    });

    const projectRevision = controller.markDirty({ value: 1 });
    const commit = await controller.beginCommit(projectRevision);
    controller.markDirty({ value: 2 });
    commit.commit();

    expect(writes).toEqual([]);
    expect(controller.getStatus()).toMatchObject({
      phase: "scheduled",
      dirtyRevision: 2,
      savedRevision: 1,
    });

    await controller.flush("newer-edit");
    expect(writes).toEqual([2]);
    await controller.dispose();
  });

  it("waits for an in-flight recovery write before opening the commit barrier", async () => {
    let releaseWrite!: () => void;
    let notifyStarted!: () => void;
    const writeStarted = new Promise<void>((resolve) => {
      notifyStarted = resolve;
    });
    const blockedWrite = new Promise<void>((resolve) => {
      releaseWrite = resolve;
    });
    const writes: number[] = [];
    const controller = new AutosaveController({
      delayMs: 60_000,
      maxWaitMs: 60_000,
      write: async (document: { value: number }) => {
        writes.push(document.value);
        notifyStarted();
        await blockedWrite;
      },
    });

    const projectRevision = controller.markDirty({ value: 1 });
    const flushing = controller.flush("in-flight");
    await writeStarted;

    let barrierReady = false;
    const barrierPromise = controller.beginCommit(projectRevision).then((value) => {
      barrierReady = true;
      return value;
    });
    controller.markDirty({ value: 2 });
    await Promise.resolve();
    expect(barrierReady).toBe(false);

    releaseWrite();
    const commit = await barrierPromise;
    await flushing;
    commit.commit();
    expect(writes).toEqual([1]);

    await controller.flush("newer-edit");
    expect(writes).toEqual([1, 2]);
    await controller.dispose();
  });
});
