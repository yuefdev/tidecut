import {
  AutosaveController,
  bindAutosaveLifecycle,
  type AutosaveStatus,
} from "./autosave";
import {
  loadProject,
  loadRecoverySnapshot,
  saveProject,
  writeRecoverySnapshot,
} from "./client";
import {
  isProjectDocument,
  type ProjectDocument,
  type SaveProjectResult,
} from "./types";

export interface ProjectSessionState<TTimeline = unknown, TSettings = unknown> {
  document: ProjectDocument<TTimeline, TSettings>;
  path?: string;
  checksum?: string;
  dirty: boolean;
  saveStatus: AutosaveStatus;
  recoveredFromBackup: boolean;
}

export interface ProjectSessionOptions<TTimeline, TSettings> {
  document: ProjectDocument<TTimeline, TSettings>;
  path?: string;
  checksum?: string;
  autosaveDelayMs?: number;
  autosaveMaxWaitMs?: number;
  onChange?: (state: ProjectSessionState<TTimeline, TSettings>) => void;
}

export class ProjectSession<TTimeline = unknown, TSettings = unknown> {
  private state: ProjectSessionState<TTimeline, TSettings>;
  private readonly onChange?: (
    state: ProjectSessionState<TTimeline, TSettings>,
  ) => void;
  private readonly autosave: AutosaveController<
    ProjectDocument<TTimeline, TSettings>
  >;
  private readonly unbindLifecycle: () => void;

  constructor(options: ProjectSessionOptions<TTimeline, TSettings>) {
    this.onChange = options.onChange;
    this.state = {
      document: cloneProjectData(options.document),
      path: options.path,
      checksum: options.checksum,
      dirty: false,
      saveStatus: {
        phase: "idle",
        dirtyRevision: 0,
        savedRevision: 0,
      },
      recoveredFromBackup: false,
    };
    this.autosave = new AutosaveController({
      delayMs: options.autosaveDelayMs,
      maxWaitMs: options.autosaveMaxWaitMs,
      write: (document, reason) =>
        writeRecoverySnapshot({
          projectId: document.projectId,
          document,
          originalPath: this.state.path,
          baseChecksum: this.state.checksum,
          recoveredFromBackup: this.state.recoveredFromBackup,
          reason,
        }),
      onStatus: (saveStatus) => {
        this.state = { ...this.state, saveStatus };
        this.emit();
      },
    });
    this.unbindLifecycle = bindAutosaveLifecycle(this.autosave);
  }

  static async open<TTimeline = unknown, TSettings = unknown>(
    path: string,
    options: Omit<
      ProjectSessionOptions<TTimeline, TSettings>,
      "document" | "path" | "checksum"
    > = {},
  ): Promise<ProjectSession<TTimeline, TSettings>> {
    const loaded = await loadProject<ProjectDocument<TTimeline, TSettings>>(path);
    assertProjectDocument(loaded.document);
    const session = new ProjectSession({
      ...options,
      document: loaded.document,
      path: loaded.path,
      checksum: loaded.checksum,
    });
    session.state = {
      ...session.state,
      recoveredFromBackup: loaded.recoveredFromBackup,
      dirty: loaded.recoveredFromBackup,
    };
    session.emit();
    return session;
  }

  static async recover<TTimeline = unknown, TSettings = unknown>(
    snapshotId: string,
    options: Omit<
      ProjectSessionOptions<TTimeline, TSettings>,
      "document" | "path" | "checksum"
    > = {},
  ): Promise<ProjectSession<TTimeline, TSettings>> {
    const loaded = await loadRecoverySnapshot<
      ProjectDocument<TTimeline, TSettings>
    >(snapshotId);
    assertProjectDocument(loaded.document);
    const session = new ProjectSession({
      ...options,
      document: loaded.document,
      path: loaded.originalPath,
      checksum: loaded.baseChecksum,
    });
    session.state = {
      ...session.state,
      dirty: true,
      recoveredFromBackup: loaded.recoveredFromBackup,
    };
    session.emit();
    return session;
  }

  get snapshot(): ProjectSessionState<TTimeline, TSettings> {
    return cloneProjectData(this.state);
  }

  update(
    updater: (
      document: ProjectDocument<TTimeline, TSettings>,
    ) => ProjectDocument<TTimeline, TSettings>,
  ): void {
    const updated = cloneProjectData(updater(cloneProjectData(this.state.document)));
    assertProjectDocument(updated);
    updated.updatedAtMs = Date.now();
    this.state = { ...this.state, document: updated, dirty: true };
    this.autosave.markDirty(updated);
    this.emit();
  }

  replace(document: ProjectDocument<TTimeline, TSettings>): void {
    const updated = cloneProjectData(document);
    assertProjectDocument(updated);
    this.state = { ...this.state, document: updated, dirty: true };
    this.autosave.markDirty(updated);
    this.emit();
  }

  async save(path = this.state.path): Promise<void> {
    if (!path) throw new Error("A project path is required for the first save");
    const stateAtStart = this.state;
    const targetRevision = this.autosave.currentRevision;
    const document = {
      ...stateAtStart.document,
      updatedAtMs: Date.now(),
    };
    const commit = await this.autosave.beginCommit(targetRevision);
    let saved: SaveProjectResult;
    try {
      saved = await saveProject({
        path,
        document,
        projectId: document.projectId,
        expectedChecksum:
          path === stateAtStart.path && !stateAtStart.recoveredFromBackup
            ? stateAtStart.checksum
            : undefined,
        // Keep the last known-good .bak when the primary file was corrupt.
        createBackup: !stateAtStart.recoveredFromBackup,
      });
    } catch (error) {
      commit.rollback();
      throw error;
    }

    const hasNewerEdits = this.autosave.currentRevision > targetRevision;
    this.state = {
      ...this.state,
      document: hasNewerEdits ? this.state.document : document,
      path: saved.path,
      checksum: saved.checksum,
      dirty: hasNewerEdits,
      recoveredFromBackup: false,
    };
    commit.commit();
    this.emit();
  }

  async flushRecovery(reason = "manual"): Promise<void> {
    await this.autosave.flush(reason);
  }

  async dispose(options: { flush?: boolean } = {}): Promise<void> {
    this.unbindLifecycle();
    await this.autosave.dispose({ flush: options.flush ?? this.state.dirty });
  }

  private emit(): void {
    this.onChange?.(this.snapshot);
  }
}

function assertProjectDocument(value: unknown): asserts value is ProjectDocument {
  if (!isProjectDocument(value)) {
    throw new Error("The file does not contain a supported Astral Lunar project");
  }
}

/**
 * Project documents are a JSON-safe persistence contract, but Svelte may wrap
 * their arrays and objects in reactive Proxy values. Browsers intentionally
 * reject Proxy objects in structuredClone, so fall back to their JSON form at
 * the project-session boundary instead of leaking reactive identities into
 * autosave, undo, or persistence snapshots.
 */
function cloneProjectData<T>(value: T): T {
  if (typeof structuredClone === "function") {
    try {
      return structuredClone(value);
    } catch {
      // JSON serialization unwraps Svelte proxies used by the editor state.
    }
  }
  return JSON.parse(JSON.stringify(value)) as T;
}
