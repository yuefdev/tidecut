import { beforeEach, describe, expect, it, vi } from "vitest";

const client = vi.hoisted(() => ({
  loadProject: vi.fn(),
  loadRecoverySnapshot: vi.fn(),
  saveProject: vi.fn(),
  writeRecoverySnapshot: vi.fn(),
}));

vi.mock("./client", () => client);

import { ProjectSession } from "./session";
import { createProjectDocument } from "./types";

function document(value = 0) {
  return createProjectDocument({
    now: 10,
    projectId: "project-1",
    name: "Edit",
    timeline: { value },
  });
}

describe("ProjectSession durable save coordination", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    client.writeRecoverySnapshot.mockResolvedValue({});
    client.saveProject.mockResolvedValue({
      path: "C:/projects/edit.astral",
      checksum: "sha256:saved",
      bytesWritten: 100,
      savedAtMs: 20,
    });
  });

  it("restores the original path and base checksum from recovery metadata", async () => {
    client.loadRecoverySnapshot.mockResolvedValue({
      path: "C:/app/recovery/project-1.autosave",
      document: document(7),
      checksum: "sha256:recovery",
      savedAtMs: 15,
      verified: true,
      recoveredFromBackup: false,
      originalPath: "C:/projects/edit.astral",
      baseChecksum: "sha256:base",
      recoverySnapshotId: "project-1",
    });

    const session = await ProjectSession.recover<{ value: number }>("project-1", {
      autosaveDelayMs: 60_000,
      autosaveMaxWaitMs: 60_000,
    });

    expect(session.snapshot).toMatchObject({
      path: "C:/projects/edit.astral",
      checksum: "sha256:base",
      dirty: true,
    });
    await session.dispose();
  });

  it("does not recreate a stale recovery snapshot after a successful manual save", async () => {
    const session = new ProjectSession({
      document: document(),
      autosaveDelayMs: 60_000,
      autosaveMaxWaitMs: 60_000,
    });
    session.update((current) => ({
      ...current,
      timeline: { value: 1 },
    }));

    await session.save("C:/projects/edit.astral");
    await session.flushRecovery("after-project-save");

    expect(client.writeRecoverySnapshot).not.toHaveBeenCalled();
    expect(session.snapshot).toMatchObject({
      path: "C:/projects/edit.astral",
      checksum: "sha256:saved",
      dirty: false,
    });
    await session.dispose();
  });

  it("keeps edits made during a project save dirty and recovery-protected", async () => {
    let releaseSave!: () => void;
    const saveBlocked = new Promise<void>((resolve) => {
      releaseSave = resolve;
    });
    client.saveProject.mockImplementationOnce(async () => {
      await saveBlocked;
      return {
        path: "C:/projects/edit.astral",
        checksum: "sha256:saved",
        bytesWritten: 100,
        savedAtMs: 20,
      };
    });

    const session = new ProjectSession({
      document: document(),
      autosaveDelayMs: 60_000,
      autosaveMaxWaitMs: 60_000,
    });
    session.update((current) => ({
      ...current,
      timeline: { value: 1 },
    }));
    const saving = session.save("C:/projects/edit.astral");
    await vi.waitFor(() => expect(client.saveProject).toHaveBeenCalledOnce());

    session.update((current) => ({
      ...current,
      timeline: { value: 2 },
    }));
    releaseSave();
    await saving;

    expect(session.snapshot.document.timeline).toEqual({ value: 2 });
    expect(session.snapshot.dirty).toBe(true);
    await session.flushRecovery("newer-edit");
    expect(client.writeRecoverySnapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        originalPath: "C:/projects/edit.astral",
        baseChecksum: "sha256:saved",
        document: expect.objectContaining({ timeline: { value: 2 } }),
      }),
    );
    await session.dispose();
  });

  it("can abandon a dirty session without writing a recovery snapshot", async () => {
    const session = new ProjectSession({
      document: document(),
      autosaveDelayMs: 60_000,
      autosaveMaxWaitMs: 60_000,
    });
    session.update((current) => ({
      ...current,
      timeline: { value: 9 },
    }));

    await session.dispose({ flush: false });

    expect(client.writeRecoverySnapshot).not.toHaveBeenCalled();
  });

  it("unwraps reactive proxy arrays before snapshot and autosave cloning", async () => {
    const reactiveSettings = new Proxy(
      { captions: new Proxy(["Merhaba"], {}) },
      {},
    );
    const reactiveDocument = new Proxy(
      { ...document(), settings: reactiveSettings },
      {},
    );
    const session = new ProjectSession({
      document: reactiveDocument,
      autosaveDelayMs: 60_000,
      autosaveMaxWaitMs: 60_000,
    });

    expect(session.snapshot.document.settings).toEqual({ captions: ["Merhaba"] });
    session.update((current) => ({
      ...current,
      settings: new Proxy(
        { captions: new Proxy(["Merhaba", "OpenAI"], {}) },
        {},
      ),
    }));
    expect(session.snapshot.document.settings).toEqual({
      captions: ["Merhaba", "OpenAI"],
    });

    await session.flushRecovery("proxy-regression");
    expect(client.writeRecoverySnapshot).toHaveBeenCalledWith(
      expect.objectContaining({
        document: expect.objectContaining({
          settings: { captions: ["Merhaba", "OpenAI"] },
        }),
      }),
    );
    await session.dispose();
  });
});
